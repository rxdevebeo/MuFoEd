//! Bounded decoding of RGB axial shadings: the ones the document writer emits,
//! and the stitched and sampled functions design tools and Word write (G2).
use crate::content::{AxialGradient, Rgb};
use crate::fonts::Resolver;
use lopdf::{Dictionary, Object};

fn number(value: &Object) -> Option<f64> {
    let value = match value {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => f64::from(*value),
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn numbers(resolve: Resolver<'_>, dict: &Dictionary, key: &[u8]) -> Option<Vec<f64>> {
    resolve
        .get(dict.get(key).ok()?)?
        .as_array()
        .ok()?
        .iter()
        .map(number)
        .collect()
}

fn colour(resolve: Resolver<'_>, dict: &Dictionary, key: &[u8]) -> Option<Rgb> {
    let values = numbers(resolve, dict, key)?;
    let &[r, g, b] = values.as_slice() else {
        return None;
    };
    values
        .iter()
        .all(|value| (0.0..=1.0).contains(value))
        .then_some(Rgb(r, g, b))
}

/// The most a sampled function's table may hold, decoded; a real one is a few
/// kilobytes (512 RGB samples of 8 bits is 1.5 KB).
const MAX_SAMPLE_BYTES: usize = 1 << 20;
/// How many stops a sampled function becomes: evenly along the axis, which a
/// DrawingML gradient interpolates between as the table would.
const SAMPLED_STOPS: usize = 9;

/// A one-input, three-output sampled function (`/FunctionType 0`) as stops.
fn sampled_stops(object: &Object, resolve: Resolver<'_>) -> Option<Vec<(f64, Rgb)>> {
    let stream = resolve.get(object)?.as_stream().ok()?;
    let dict = &stream.dict;
    let size = numbers(resolve, dict, b"Size")?;
    let &[count] = size.as_slice() else {
        return None;
    };
    let count = count as usize;
    let bits = number(dict.get(b"BitsPerSample").ok()?)? as u32;
    let width = match bits {
        8 => 1,
        16 => 2,
        _ => return None,
    };
    if !(2..=65_536).contains(&count)
        || numbers(resolve, dict, b"Domain")? != [0.0, 1.0]
        || numbers(resolve, dict, b"Range")?.len() != 6
    {
        return None;
    }
    // The last index; `count` is at least 2.
    let last = count.saturating_sub(1);
    // `/Encode` and `/Decode` other than their defaults remap the table; only
    // the defaults are read.
    if let Some(encode) = numbers(resolve, dict, b"Encode") {
        if encode != [0.0, last as f64] {
            return None;
        }
    }
    if let Some(decode) = numbers(resolve, dict, b"Decode") {
        if decode != [0.0, 1.0, 0.0, 1.0, 0.0, 1.0] {
            return None;
        }
    }
    let bytes = crate::document::bounded_decompress(stream, MAX_SAMPLE_BYTES).ok()?;
    // 255 or 65 535: the largest sample of the width.
    let max = if width == 1 { 255.0 } else { 65_535.0 };
    let sample = |index: usize, channel: usize| -> Option<f64> {
        let at = index
            .checked_mul(3)?
            .checked_add(channel)?
            .checked_mul(width)?;
        let value = match width {
            1 => u32::from(*bytes.get(at)?),
            _ => u32::from(u16::from_be_bytes([
                *bytes.get(at)?,
                *bytes.get(at.checked_add(1)?)?,
            ])),
        };
        Some(f64::from(value) / max)
    };
    (0..SAMPLED_STOPS)
        .map(|step| {
            let t = step as f64 / SAMPLED_STOPS.saturating_sub(1) as f64;
            let index = ((t * last as f64).round() as usize).min(last);
            Some((
                t,
                Rgb(sample(index, 0)?, sample(index, 1)?, sample(index, 2)?),
            ))
        })
        .collect()
}

/// The colour `t` of the way from `a` to `b`.
fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    Rgb(
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}

fn interpolation(resolve: Resolver<'_>, object: &Object) -> Option<(Rgb, Rgb)> {
    let dict = resolve.get(object)?.as_dict().ok()?;
    if dict.get(b"FunctionType").ok()?.as_i64().ok()? != 2
        || numbers(resolve, dict, b"Domain")? != [0.0, 1.0]
        || (number(dict.get(b"N").ok()?)? - 1.0).abs() > f64::EPSILON
    {
        return None;
    }
    Some((colour(resolve, dict, b"C0")?, colour(resolve, dict, b"C1")?))
}

/// `DeviceRGB`, or an ICC-based space of three components - how design
/// tools and browsers write their gradients' colour.
fn is_rgb_space(resolve: Resolver<'_>, space: &Object) -> bool {
    match space {
        Object::Name(name) => name == b"DeviceRGB",
        Object::Array(items) => {
            let [Object::Name(kind), profile] = items.as_slice() else {
                return false;
            };
            kind == b"ICCBased"
                && resolve
                    .get(profile)
                    .and_then(|profile| profile.as_stream().ok())
                    .and_then(|profile| profile.dict.get(b"N").ok())
                    .and_then(|n| n.as_i64().ok())
                    == Some(3)
        }
        _ => false,
    }
}

pub(crate) fn decode(resolve: Resolver<'_>, object: &Object) -> Option<AxialGradient> {
    let dict = resolve.get(object)?.as_dict().ok()?;
    if dict.get(b"ShadingType").ok()?.as_i64().ok()? != 2
        || !is_rgb_space(resolve, resolve.get(dict.get(b"ColorSpace").ok()?)?)
    {
        return None;
    }
    // `/Extend` only says whether the colour runs on past the axis's ends;
    // a gradient that stops there is still that gradient between them.
    let coords = numbers(resolve, dict, b"Coords")?;
    let &[x1, y1, x2, y2] = coords.as_slice() else {
        return None;
    };
    if (x2 - x1).hypot(y2 - y1) <= f64::EPSILON {
        return None;
    }
    let function_object = dict.get(b"Function").ok()?;
    // A sampled function is a stream; the others are dictionaries.
    let function = match resolve.get(function_object)? {
        Object::Dictionary(dict) => dict,
        Object::Stream(stream) => &stream.dict,
        _ => return None,
    };
    let kind = function.get(b"FunctionType").ok()?.as_i64().ok()?;
    let stops = if kind == 2 {
        let (first, last) = interpolation(resolve, function_object)?;
        vec![(0.0, first), (1.0, last)]
    } else if kind == 3 {
        if numbers(resolve, function, b"Domain")? != [0.0, 1.0] {
            return None;
        }
        let functions = resolve
            .get(function.get(b"Functions").ok()?)?
            .as_array()
            .ok()?;
        if functions.is_empty() || functions.len() > 4096 {
            return None;
        }
        let bounds = numbers(resolve, function, b"Bounds")?;
        if bounds.len().checked_add(1)? != functions.len()
            || bounds.iter().any(|value| *value <= 0.0 || *value >= 1.0)
            || bounds
                .windows(2)
                .any(|pair| matches!(pair, [a, b] if a >= b))
        {
            return None;
        }
        // `/Encode` maps each piece's share of the axis onto its own function's
        // domain; `[1 0]` runs a piece backwards, from `C1` to `C0`, which is
        // how Word writes a gradient whose stops it reversed (G2).
        let encode = numbers(resolve, function, b"Encode")?;
        if encode.len() != functions.len().checked_mul(2)?
            || encode.iter().any(|value| !(0.0..=1.0).contains(value))
        {
            return None;
        }
        let starts = std::iter::once(0.0).chain(bounds.iter().copied());
        let ends = bounds.iter().copied().chain([1.0]);
        let mut stops: Vec<(f64, Rgb)> = Vec::with_capacity(functions.len().checked_mul(2)?);
        for (((function, start), end), pair) in functions
            .iter()
            .zip(starts)
            .zip(ends)
            .zip(encode.chunks_exact(2))
        {
            let (c0, c1) = interpolation(resolve, function)?;
            let &[e0, e1] = pair else {
                return None;
            };
            let (first, last) = (mix(c0, c1, e0), mix(c0, c1, e1));
            // A piece that does not start where the last one ended is a hard
            // edge: two stops at one place.
            if stops.last().is_none_or(|(_, previous)| *previous != first) {
                stops.push((start, first));
            }
            stops.push((end, last));
        }
        stops
    } else if kind == 0 {
        sampled_stops(function_object, resolve)?
    } else {
        return None;
    };
    Some(AxialGradient {
        stops,
        start: (x1, y1),
        end: (x2, y2),
    })
}

#[cfg(test)]
mod tests {
    use super::decode;
    use crate::fonts::resolver;
    use lopdf::{dictionary, Document, Object};

    fn sample(exponent: i64, space: &[u8], coords: &[f32]) -> Object {
        let floats =
            |values: &[f32]| Object::Array(values.iter().copied().map(Object::Real).collect());
        let function = dictionary! {
            "FunctionType" => 2,
            "Domain" => floats(&[0.0, 1.0]),
            "C0" => floats(&[1.0, 0.0, 0.0]),
            "C1" => floats(&[0.0, 0.0, 1.0]),
            "N" => exponent,
        };
        Object::Dictionary(dictionary! {
            "ShadingType" => 2,
            "ColorSpace" => Object::Name(space.to_vec()),
            "Coords" => floats(coords),
            "Extend" => vec![Object::Boolean(true), Object::Boolean(true)],
            "Function" => Object::Dictionary(function),
        })
    }

    /// A sampled function (Word's gradients) is read as evenly spaced stops.
    #[test]
    fn a_sampled_function_becomes_stops() {
        let document = Document::with_version("1.7");
        // Red to blue over 3 samples.
        let samples = vec![255, 0, 0, 128, 0, 128, 0, 0, 255];
        let floats =
            |values: &[f32]| Object::Array(values.iter().copied().map(Object::Real).collect());
        let function = lopdf::Stream::new(
            dictionary! {
                "FunctionType" => 0,
                "Size" => vec![Object::Integer(3)],
                "BitsPerSample" => 8,
                "Domain" => floats(&[0.0, 1.0]),
                "Range" => floats(&[0.0, 1.0, 0.0, 1.0, 0.0, 1.0]),
            },
            samples,
        );
        let shading = Object::Dictionary(dictionary! {
            "ShadingType" => 2,
            "ColorSpace" => Object::Name(b"DeviceRGB".to_vec()),
            "Coords" => floats(&[0.0, 0.0, 100.0, 0.0]),
            "Function" => Object::Stream(function),
        });
        let gradient = decode(resolver(&document), &shading).expect("a gradient");
        assert_eq!(gradient.stops.len(), 9);
        let first = gradient.stops.first().expect("a stop").1;
        let last = gradient.stops.last().expect("a stop").1;
        assert!((first.0 - 1.0).abs() < 1e-9 && first.2.abs() < 1e-9);
        assert!(last.0.abs() < 1e-9 && (last.2 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn only_resolved_linear_rgb_functions_with_a_real_axis_are_carried() {
        let document = Document::with_version("1.7");
        let coords = [0_f32, 0.0, 100.0, 0.0];
        assert!(decode(resolver(&document), &sample(1, b"DeviceRGB", &coords)).is_some());
        assert!(decode(resolver(&document), &sample(2, b"DeviceRGB", &coords)).is_none());
        assert!(decode(resolver(&document), &sample(1, b"DeviceCMYK", &coords)).is_none());
        assert!(decode(resolver(&document), &sample(1, b"DeviceRGB", &[0.0; 4])).is_none());
        assert!(decode(
            resolver(&document),
            &sample(1, b"DeviceRGB", &[f32::INFINITY; 4])
        )
        .is_none());
    }

    #[test]
    fn an_icc_based_rgb_space_and_no_extend_are_carried() {
        let mut document = Document::with_version("1.7");
        let profile = document.add_object(lopdf::Stream::new(dictionary! { "N" => 3 }, Vec::new()));
        let Object::Dictionary(mut shading) = sample(1, b"DeviceRGB", &[0.0, 0.0, 100.0, 0.0])
        else {
            unreachable!()
        };
        shading.set(
            "ColorSpace",
            Object::Array(vec![
                Object::Name(b"ICCBased".to_vec()),
                Object::Reference(profile),
            ]),
        );
        shading.remove(b"Extend");
        assert!(decode(resolver(&document), &Object::Dictionary(shading)).is_some());
    }
}
