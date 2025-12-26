use super::*;

#[test]
fn log2_floor_handles_gt_one() {
	let val = BigRational::new(BigInt::from(3), BigInt::from(2));
	assert_eq!(log2_floor(&val), 0);
}

#[test]
fn fp32_roundtrip_for_one_point_five() {
	let spec = FloatSpec {
		name: "FP32",
		exponent_bits: 8,
		significand_bits: 23,
	};
	let parsed = ParsedValue::Finite(BigRational::new(BigInt::from(3), BigInt::from(2)));
	let soft = parsed_to_softfloat(&parsed, &spec, RoundingMode::HalfEven);
	assert_eq!(soft.class, Class::Normal);
	assert_eq!(soft.exponent, 0);
	let bits = softfloat_to_bits(&soft, &spec);
	assert_eq!(bits, "00111111110000000000000000000000");
}

#[test]
fn bits_input_allows_0b_prefix() {
	let spec = FloatSpec {
		name: "FP32",
		exponent_bits: 8,
		significand_bits: 23,
	};
	let parsed =
		bits_to_softfloat("0b00111111110000000000000000000000", &spec).expect("parse bits");
	assert_eq!(parsed.class, Class::Normal);
	assert_eq!(parsed.exponent, 0);
}

#[test]
fn hex_input_allows_0x_prefix() {
	let spec = FloatSpec {
		name: "FP32",
		exponent_bits: 8,
		significand_bits: 23,
	};
	let bits = hex_to_bits("0X3FC00000", total_bits(&spec).unwrap()).expect("hex to bits");
	assert_eq!(bits, "00111111110000000000000000000000");
}

#[test]
fn decimal_zero_point_one_matches_reference_bits() {
	let spec = FloatSpec {
		name: "FP32",
		exponent_bits: 8,
		significand_bits: 23,
	};
	let parsed = parse_decimal("0.1").expect("parse decimal");
	let soft = parsed_to_softfloat(&parsed, &spec, RoundingMode::HalfEven);
	let bits = softfloat_to_bits(&soft, &spec);
	assert_eq!(bits, "00111101110011001100110011001101");
}

#[test]
fn decimal_negative_two_point_five_matches_reference_bits() {
	let spec = FloatSpec {
		name: "FP32",
		exponent_bits: 8,
		significand_bits: 23,
	};
	let parsed = parse_decimal("-2.5").expect("parse decimal");
	let soft = parsed_to_softfloat(&parsed, &spec, RoundingMode::HalfEven);
	let bits = softfloat_to_bits(&soft, &spec);
	assert_eq!(bits, "11000000001000000000000000000000");
}
