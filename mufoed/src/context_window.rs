//! How many tokens to ask Ollama for.
//!
//! Ollama's own default window is a few thousand tokens. A model often allows
//! far more, and the limit that actually fits is the KV cache left in video
//! memory after the weights. The figure is measured once per model and sent as
//! `options.num_ctx`.

use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// Used when the model card or the GPU cannot be read.
const FALLBACK: u32 = 16_384;

/// Left free for the desktop and the compute graph, in bytes.
const RESERVE: u64 = 1536 * 1024 * 1024;

/// The window for `model` at `endpoint`, cached for the process.
pub(crate) fn num_ctx(endpoint: &str, model: &str) -> u32 {
    let key = format!("{}|{model}", endpoint.trim_end_matches('/'));
    if let Some(found) = cache().lock().ok().and_then(|map| map.get(&key).copied()) {
        return found;
    }
    let fitted = measure(endpoint, model);
    if let Ok(mut map) = cache().lock() {
        map.insert(key, fitted);
    }
    fitted
}

fn cache() -> &'static Mutex<std::collections::HashMap<String, u32>> {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, u32>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn measure(endpoint: &str, model: &str) -> u32 {
    let Some(show) = post_json(
        &format!("{}/api/show", endpoint.trim_end_matches('/')),
        &serde_json::json!({"model": model}),
    ) else {
        return FALLBACK;
    };
    let weight = tags_size(endpoint, model)
        .or_else(|| estimated_weight(&show))
        .unwrap_or(0);
    let Some(card) = card_of(&show, weight) else {
        return FALLBACK;
    };
    fit_context(
        card.native,
        card.kv_per_token,
        card.weight,
        dedicated_vram(),
    )
}

struct Card {
    native: u32,
    kv_per_token: u64,
    weight: u64,
}

fn card_of(show: &Value, weight: u64) -> Option<Card> {
    let info = show.get("model_info")?.as_object()?;
    let (prefix, native) = info
        .iter()
        .filter_map(|(key, value)| {
            let prefix = key.strip_suffix(".context_length")?;
            let native = value.as_u64()?;
            Some((prefix.to_owned(), native))
        })
        .max_by_key(|(_, native)| *native)?;
    let layers = number(info, &prefix, "block_count").unwrap_or(0);
    let kv_heads = number(info, &prefix, "attention.head_count_kv").unwrap_or(0);
    let key = number(info, &prefix, "attention.key_length").unwrap_or(0);
    let value = number(info, &prefix, "attention.value_length").unwrap_or(key);
    // K and V are stored at f16 unless the runner is told otherwise.
    let kv_per_token = layers
        .saturating_mul(kv_heads)
        .saturating_mul(key.saturating_add(value))
        * 2;
    let native = u32::try_from(native).unwrap_or(u32::MAX);
    Some(Card {
        native,
        kv_per_token,
        weight,
    })
}

fn number(info: &serde_json::Map<String, Value>, prefix: &str, suffix: &str) -> Option<u64> {
    info.get(&format!("{prefix}.{suffix}"))
        .and_then(Value::as_u64)
}

fn tags_size(endpoint: &str, model: &str) -> Option<u64> {
    let tags = get_json(&format!("{}/api/tags", endpoint.trim_end_matches('/')))?;
    tags.get("models")?.as_array()?.iter().find_map(|entry| {
        (entry.get("name").and_then(Value::as_str) == Some(model))
            .then(|| entry.get("size").and_then(Value::as_u64))
            .flatten()
    })
}

fn estimated_weight(show: &Value) -> Option<u64> {
    let details = show.get("details")?;
    let params = parse_params(details.get("parameter_size").and_then(Value::as_str)?)?;
    let millis = quant_millis(
        details
            .get("quantization_level")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    Some(params.saturating_mul(millis) / 1000)
}

fn parse_params(text: &str) -> Option<u64> {
    let text = text.trim();
    let (number, mult) = if let Some(rest) = text.strip_suffix(['B', 'b']) {
        (rest, 1_000_000_000u64)
    } else if let Some(rest) = text.strip_suffix(['M', 'm']) {
        (rest, 1_000_000)
    } else {
        return None;
    };
    let (whole, frac) = number.split_once('.').unwrap_or((number, ""));
    let whole: u64 = whole.parse().ok()?;
    let digits: String = frac.chars().take(3).collect();
    let scale = 10u64.pow(u32::try_from(digits.len()).unwrap_or(0));
    let frac_v: u64 = if digits.is_empty() {
        0
    } else {
        digits.parse().ok()?
    };
    Some(
        whole
            .saturating_mul(mult)
            .saturating_add(frac_v.saturating_mul(mult / scale.max(1))),
    )
}

/// Bytes per parameter, times 1000, so the product stays an integer.
fn quant_millis(quant: &str) -> u64 {
    let quant = quant.to_lowercase();
    if quant.contains("q2") {
        350
    } else if quant.contains("q3") || quant.contains("iq3") {
        450
    } else if quant.contains("q5") {
        700
    } else if quant.contains("q6") {
        820
    } else if quant.contains("q8") {
        1000
    } else if quant.contains("f16") || quant.contains("bf16") {
        2000
    } else if quant.contains("f32") {
        4000
    } else {
        // Q4 and the unknown quants sit near half a byte per weight.
        580
    }
}

/// The largest window that fits, in tokens.
///
/// `native` is the model's own limit. `kv_per_token` is the f16 K+V cache for
/// one token. `weight` is the GGUF size. `vram` is dedicated video memory;
/// without it the window stays at 16 384 so a 256k model is not loaded blind.
pub(crate) fn fit_context(native: u32, kv_per_token: u64, weight: u64, vram: Option<u64>) -> u32 {
    let native = if native == 0 { FALLBACK } else { native };
    let Some(vram) = vram else {
        return native.min(FALLBACK);
    };
    if weight == 0 || kv_per_token == 0 {
        return native.min(FALLBACK);
    }
    let room = vram.saturating_sub(weight).saturating_sub(RESERVE);
    let tokens = u32::try_from(room / kv_per_token).unwrap_or(u32::MAX);
    let fitted = tokens.min(native);
    let aligned = (fitted / 1024) * 1024;
    aligned.max(2048).min(native)
}

fn dedicated_vram() -> Option<u64> {
    #[cfg(windows)]
    {
        vram_command("nvidia-smi").or_else(|| {
            let path = std::env::var_os("ProgramFiles")?;
            vram_command(
                std::path::PathBuf::from(path)
                    .join("NVIDIA Corporation")
                    .join("NVSMI")
                    .join("nvidia-smi.exe"),
            )
        })
    }
    #[cfg(not(windows))]
    None
}

#[cfg(windows)]
fn vram_command(program: impl AsRef<std::ffi::OsStr>) -> Option<u64> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new(program)
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    text.lines()
        .filter_map(|line| {
            let digits: String = line.chars().filter(|c| c.is_ascii_digit()).collect();
            digits
                .parse::<u64>()
                .ok()
                .map(|mib| mib.saturating_mul(1024 * 1024))
        })
        .max()
}

fn http() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(5)))
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn get_json(url: &str) -> Option<Value> {
    let mut response = http().get(url).call().ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    let text = response.body_mut().read_to_string().ok()?;
    serde_json::from_str(&text).ok()
}

fn post_json(url: &str, body: &Value) -> Option<Value> {
    let mut response = http()
        .post(url)
        .header("content-type", "application/json")
        .send(body.to_string())
        .ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    let text = response.body_mut().read_to_string().ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_16gb_card_fits_qwen35_9b_at_64k() {
        // 33 layers, 4 KV heads, key and value 256, f16: 135168 bytes a token.
        // Weights 6603314868. Card 16303 MiB. Reserve 1.5 GiB.
        let tokens = fit_context(262_144, 135_168, 6_603_314_868, Some(16_303 * 1024 * 1024));
        assert_eq!(tokens, 65_536);
    }

    #[test]
    fn without_video_memory_the_window_stays_modest() {
        assert_eq!(fit_context(262_144, 135_168, 6_000_000_000, None), 16_384);
    }

    #[test]
    fn the_models_own_limit_wins_when_memory_is_ample() {
        assert_eq!(
            fit_context(4096, 1000, 1_000_000_000, Some(24_u64 << 30)),
            4096
        );
    }

    #[test]
    fn a_full_card_still_gets_a_small_window() {
        assert_eq!(
            fit_context(32_768, 135_168, 8_000_000_000, Some(8_000_000_000)),
            2048
        );
    }

    #[test]
    fn parameter_size_and_quant_estimate_the_file() {
        let show = serde_json::json!({
            "details": {"parameter_size": "9.7B", "quantization_level": "Q4_K_M"}
        });
        let weight = estimated_weight(&show).unwrap();
        assert!(weight > 5_000_000_000 && weight < 7_000_000_000, "{weight}");
    }
}
