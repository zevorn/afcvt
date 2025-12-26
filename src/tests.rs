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
