//! Bounded decoding of RGB axial shadings emitted by the document writer.
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

pub(crate) fn decode(resolve: Resolver<'_>, object: &Object) -> Option<AxialGradient> {
    let dict = resolve.get(object)?.as_dict().ok()?;
    if dict.get(b"ShadingType").ok()?.as_i64().ok()? != 2
        || resolve.get(dict.get(b"ColorSpace").ok()?)?.as_name().ok()? != b"DeviceRGB"
    {
        return None;
    }
    let extend = resolve.get(dict.get(b"Extend").ok()?)?.as_array().ok()?;
    if !matches!(
        extend.as_slice(),
        [Object::Boolean(true), Object::Boolean(true)]
    ) {
        return None;
    }
    let coords = numbers(resolve, dict, b"Coords")?;
    let &[x1, y1, x2, y2] = coords.as_slice() else {
        return None;
    };
    if (x2 - x1).hypot(y2 - y1) <= f64::EPSILON {
        return None;
    }
    let function_object = dict.get(b"Function").ok()?;
    let function = resolve.get(function_object)?.as_dict().ok()?;
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
        let encode = numbers(resolve, function, b"Encode")?;
        if encode.len() != functions.len().checked_mul(2)?
            || encode.chunks_exact(2).any(|pair| pair != [0.0, 1.0])
        {
            return None;
        }
        let ends = bounds.into_iter().chain([1.0]);
        let mut stops = Vec::with_capacity(functions.len().checked_add(1)?);
        for (function, end) in functions.iter().zip(ends) {
            let (first, last) = interpolation(resolve, function)?;
            if let Some((_, previous)) = stops.last() {
                if *previous != first {
                    return None;
                }
            } else {
                stops.push((0.0, first));
            }
            stops.push((end, last));
        }
        stops
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
}
