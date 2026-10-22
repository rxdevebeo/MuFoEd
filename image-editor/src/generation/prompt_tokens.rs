//! Full CLIP contexts: explicit BOS/EOS, weighted tokens, automatic overflow and BREAK.
use anyhow::{anyhow, bail, Result};
use candle_core::{DType, Device, Tensor};
use candle_transformers::models::stable_diffusion::clip::Config;
use tokenizers::Tokenizer;

const MAX_CONTEXTS: usize = 16;
#[derive(Clone)]
pub(super) struct Chunk {
    ids: Vec<u32>,
    weights: Vec<f32>,
}
impl Chunk {
    pub(super) fn tensors(
        &self,
        device: &Device,
        dtype: DType,
    ) -> Result<(Tensor, Option<Tensor>)> {
        let ids = Tensor::new(self.ids.as_slice(), device)?.unsqueeze(0)?;
        let weighted = self.weights.iter().any(|v| (*v - 1.0).abs() > f32::EPSILON);
        let weights = if weighted {
            Some(
                Tensor::from_vec(self.weights.clone(), (1, self.ids.len(), 1), device)?
                    .to_dtype(dtype)?,
            )
        } else {
            None
        };
        Ok((ids, weights))
    }
}
pub(super) struct Batch {
    pub(super) l: Vec<Vec<Chunk>>,
    pub(super) g: Option<Vec<Vec<Chunk>>>,
}
pub(super) fn prepare(
    l: (&Tokenizer, &Config),
    g: Option<(&Tokenizer, &Config)>,
    texts: &[&str],
) -> Result<Batch> {
    let mut left = texts
        .iter()
        .map(|text| tokenize(l.0, l.1, text))
        .collect::<Result<Vec<_>>>()?;
    let mut right = g
        .map(|(tok, cfg)| {
            texts
                .iter()
                .map(|text| tokenize(tok, cfg, text))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let count = left
        .iter()
        .chain(right.iter().flatten())
        .map(Vec::len)
        .max()
        .unwrap_or(1);
    let blank = tokenize(l.0, l.1, "")?.remove(0);
    for chunks in &mut left {
        chunks.resize(count, blank.clone());
    }
    if let (Some(right), Some((tok, cfg))) = (&mut right, g) {
        let blank = tokenize(tok, cfg, "")?.remove(0);
        for chunks in right {
            chunks.resize(count, blank.clone());
        }
    }
    Ok(Batch { l: left, g: right })
}
fn tokenize(tok: &Tokenizer, cfg: &Config, text: &str) -> Result<Vec<Chunk>> {
    let bos = tok
        .token_to_id("<|startoftext|>")
        .ok_or_else(|| anyhow!("missing CLIP BOS"))?;
    let eos = tok
        .token_to_id("<|endoftext|>")
        .ok_or_else(|| anyhow!("missing CLIP EOS"))?;
    let pad = tok
        .token_to_id(cfg.pad_with.as_deref().unwrap_or("<|endoftext|>"))
        .ok_or_else(|| anyhow!("missing CLIP padding"))?;
    let length = cfg.max_position_embeddings;
    if length < 3 {
        bail!("CLIP context cannot hold BOS, content and EOS");
    }
    let mut tok = tok.clone();
    tok.with_truncation(None)
        .map_err(|e| anyhow!(e.to_string()))?;
    tok.with_padding(None);
    let mut result = Vec::new();
    for section in plakat::prompt::break_chunks::split(text) {
        let segments = if plakat::prompt::a1111::has_attention_syntax(&section) {
            plakat::prompt::a1111::parse(&section)
        } else {
            vec![plakat::prompt::a1111::WeightedSegment {
                text: section,
                weight: 1.0,
            }]
        };
        let mut body = Vec::new();
        for segment in segments {
            if !segment.weight.is_finite() {
                bail!("Non-finite prompt weight");
            }
            let encoded = tok
                .encode(segment.text.as_str(), false)
                .map_err(|e| anyhow!(e.to_string()))?;
            body.extend(encoded.get_ids().iter().map(|&id| (id, segment.weight)));
        }
        // An empty section still contributes a correctly terminated empty CLIP context.
        let contexts = body.len().div_ceil(length - 2).max(1);
        if result.len() + contexts > MAX_CONTEXTS {
            bail!("Prompt exceeds {MAX_CONTEXTS} CLIP contexts; shorten it");
        }
        for index in 0..contexts {
            let begin = index * (length - 2);
            let end = (begin + length - 2).min(body.len());
            let mut ids = vec![bos];
            let mut weights = vec![1.0];
            for &(id, weight) in &body[begin..end] {
                ids.push(id);
                weights.push(weight);
            }
            ids.push(eos);
            weights.push(1.0);
            ids.resize(length, pad);
            weights.resize(length, 1.0);
            result.push(Chunk { ids, weights });
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn tokenizer() -> Tokenizer {
        let mut vocab: std::collections::HashMap<String, u32> = [
            ("[UNK]", 0),
            ("!", 1),
            ("<|startoftext|>", 49406),
            ("<|endoftext|>", 49407),
        ]
        .into_iter()
        .map(|(s, id)| (s.into(), id))
        .collect();
        for i in 0..180u32 {
            vocab.insert(format!("word{i}"), 100 + i);
        }
        let model = tokenizers::models::wordlevel::WordLevel::builder()
            .vocab(vocab)
            .unk_token("[UNK]".into())
            .build()
            .unwrap();
        let mut tok = Tokenizer::new(model);
        tok.with_pre_tokenizer(Some(
            tokenizers::pre_tokenizers::whitespace::WhitespaceSplit,
        ));
        tok
    }
    #[test]
    fn overflow_retains_every_token_eos_and_weights() {
        let tok = tokenizer();
        let cfg = Config::v1_5();
        let text = (0..160)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let chunks = tokenize(&tok, &cfg, &format!("({text}:1.5)")).unwrap();
        assert_eq!(chunks.len(), 3);
        let mut content = Vec::new();
        for chunk in &chunks {
            assert_eq!(chunk.ids[0], 49406);
            assert_eq!(chunk.ids.len(), 77);
            let eos = chunk.ids.iter().position(|&v| v == 49407).unwrap();
            assert!(eos > 0);
            content.extend_from_slice(&chunk.ids[1..eos]);
            assert!(chunk.weights[1..eos]
                .iter()
                .all(|v| (*v - 1.5).abs() < 1e-6));
            assert!(chunk.weights[eos..].iter().all(|v| (*v - 1.0).abs() < 1e-6));
        }
        assert_eq!(content, (100..260).collect::<Vec<u32>>());
    }
    #[test]
    fn break_and_empty_cfg_branches_align_both_encoders_with_their_own_padding() {
        let tok = tokenizer();
        let l = Config::v1_5();
        let mut g = Config::v1_5();
        g.pad_with = Some("!".into());
        let batch = prepare((&tok, &l), Some((&tok, &g)), &["", "word1 BREAK word2"]).unwrap();
        let right = batch.g.unwrap();
        assert!(batch.l.iter().chain(&right).all(|branch| branch.len() == 2));
        assert_eq!(batch.l[0][1].ids[..3], [49406, 49407, 49407]);
        assert_eq!(right[0][1].ids[..3], [49406, 49407, 1]);
        assert_eq!(right[1][1].ids[..3], [49406, 102, 49407]);
    }
    #[test]
    fn tokenizer_truncation_is_disabled_and_limits_fail_explicitly() {
        let mut tok = tokenizer();
        tok.with_truncation(Some(tokenizers::utils::truncation::TruncationParams {
            max_length: 3,
            ..Default::default()
        }))
        .unwrap();
        let cfg = Config::v1_5();
        let text = vec!["word1"; 90].join(" ");
        let chunks = tokenize(&tok, &cfg, &text).unwrap();
        assert_eq!(chunks.len(), 2);
        let oversized = vec!["word1"; 75 * MAX_CONTEXTS + 1].join(" ");
        assert!(tokenize(&tok, &cfg, &oversized).is_err());
        assert!(tokenize(&tok, &cfg, "").unwrap()[0].ids.contains(&49407));
    }
}
