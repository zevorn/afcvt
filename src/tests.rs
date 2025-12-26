use super::*;
use serde::Deserialize;
use std::process::Command;
use std::str::FromStr;

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

#[test]
fn fp16_one_point_five_matches_reference_bits() {
	let spec = FloatSpec {
		name: "FP16",
		exponent_bits: 5,
		significand_bits: 10,
	};
	let parsed = parse_decimal("1.5").expect("parse decimal");
	let soft = parsed_to_softfloat(&parsed, &spec, RoundingMode::HalfEven);
	let bits = softfloat_to_bits(&soft, &spec);
	assert_eq!(bits, "0011111000000000");
}

#[test]
fn bfloat16_pi_matches_reference_bits() {
	let spec = FloatSpec {
		name: "bfloat16",
		exponent_bits: 8,
		significand_bits: 7,
	};
	let parsed = parse_decimal("3.14159265").expect("parse decimal");
	let soft = parsed_to_softfloat(&parsed, &spec, RoundingMode::HalfEven);
	let bits = softfloat_to_bits(&soft, &spec);
	assert_eq!(bits, "0100000001001001");
}

#[test]
fn fp64_negative_value_matches_reference_bits() {
	let spec = FloatSpec {
		name: "FP64",
		exponent_bits: 11,
		significand_bits: 52,
	};
	let parsed = parse_decimal("-123.456").expect("parse decimal");
	let soft = parsed_to_softfloat(&parsed, &spec, RoundingMode::HalfEven);
	let bits = softfloat_to_bits(&soft, &spec);
	assert_eq!(
		bits,
		"1100000001011110110111010010111100011010100111111011111001110111"
	);
}

#[derive(Deserialize)]
struct ReferenceFraction {
	num: String,
	den: String,
}

#[derive(Deserialize)]
struct ReferenceSample {
	hex: String,
	bits: String,
	#[serde(rename = "type")]
	kind: u32,
	sign: bool,
	exponent: i32,
	significand: String,
	fraction: Option<ReferenceFraction>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceDump {
	format: String,
	exponent_width: usize,
	significand_width: usize,
	total_bits: usize,
	count: usize,
	samples: Vec<ReferenceSample>,
}

fn run_reference(format: &str, limit: Option<usize>) -> ReferenceDump {
	let mut cmd = Command::new("node");
	cmd.arg("scripts/fetch_flop_reference.js");
	cmd.arg(format!("--format={format}"));
	if let Some(limit) = limit {
		cmd.arg(format!("--limit={limit}"));
	}
	let output = cmd.output().expect("spawn node");
	if !output.status.success() {
		panic!(
			"reference script failed: {}",
			String::from_utf8_lossy(&output.stderr)
		);
	}
	serde_json::from_slice(&output.stdout).expect("parse reference json")
}

fn spec_from_dump(dump: &ReferenceDump) -> FloatSpec {
	let name = match dump.format.as_str() {
		"FP16" => "FP16",
		"BF16" => "bfloat16",
		"TF32" => "TensorFloat-32",
		"FP32" => "FP32",
		"FP64" => "FP64",
		other => panic!("unknown format {other}"),
	};
	FloatSpec {
		name,
		exponent_bits: dump.exponent_width,
		significand_bits: dump.significand_width,
	}
}

fn compare_against_reference(dump: ReferenceDump) {
	let spec = spec_from_dump(&dump);
	let total = dump.total_bits;
	for sample in dump.samples {
		let soft = bits_to_softfloat(&sample.bits, &spec).expect("parse bits");
		assert_eq!(soft.sign, sample.sign, "sign mismatch for hex {}", sample.hex);

		let expected_class = match sample.kind {
			0 => Class::Normal,
			1 => {
				if sample.significand == "0" {
					Class::Zero
				} else {
					Class::Subnormal
				}
			}
			2 => Class::PosInfinity,
			3 => Class::NegInfinity,
			_ => Class::Nan,
		};

		assert_eq!(
			soft.class, expected_class,
			"class mismatch for hex {}, bits {}",
			sample.hex, sample.bits
		);

		if matches!(expected_class, Class::Normal | Class::Subnormal | Class::Zero) {
			assert_eq!(
				soft.exponent, sample.exponent,
				"exponent mismatch for hex {}",
				sample.hex
			);
		}

		if let Some(fr) = sample.fraction {
			let num = BigInt::from_str(&fr.num).expect("num");
			let den = BigInt::from_str(&fr.den).expect("den");
			let reference = BigRational::new(num, den);
			let ours = softfloat_to_rational(&soft, &spec).expect("rational value");
			assert_eq!(
				ours, reference,
				"value mismatch for bits {} ({} bits expected {})",
				sample.bits, total, sample.hex
			);
		}
	}
}

#[test]
fn site_reference_fp16_full_space() {
	let dump = run_reference("FP16", None);
	assert_eq!(dump.count, 65536);
	compare_against_reference(dump);
}

#[test]
fn site_reference_bfloat16_full_space() {
	let dump = run_reference("BF16", None);
	assert_eq!(dump.count, 65536);
	compare_against_reference(dump);
}
