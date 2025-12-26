use anyhow::{Context, Result, anyhow, bail};
use bigdecimal::BigDecimal;
use clap::{Parser, ValueEnum};
use num_bigint::{BigInt, BigUint, ToBigInt};
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use std::cmp::Ordering;
use std::str::FromStr;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Rust CLI for arbitrary IEEE754-style floating-point conversion"
)]
struct Cli {
    /// Target format (built-ins or custom)
    #[arg(short, long, default_value = "fp32", value_enum)]
    format: FormatChoice,

    /// Exponent bit width (required when --format=custom)
    #[arg(long = "exp", value_name = "BITS")]
    exponent_bits: Option<usize>,

    /// Significand bit width (required when --format=custom)
    #[arg(long = "mant", value_name = "BITS")]
    significand_bits: Option<usize>,

    /// Rounding mode used when converting from decimal
    #[arg(long, default_value = "half-even", value_enum)]
    rounding: RoundingMode,

    /// Decimal digits to emit for numeric outputs
    #[arg(long, default_value_t = 32)]
    precision: usize,

    /// Use scientific notation for displayed numbers
    #[arg(long, default_value = "plain", value_enum)]
    notation: Notation,

    /// Provide a raw bit string (overrides positional decimal input)
    #[arg(long, conflicts_with = "hex")]
    bits: Option<String>,

    /// Provide a hexadecimal encoding of the bits (overrides positional decimal input)
    #[arg(long, conflicts_with = "bits")]
    hex: Option<String>,

    /// Decimal input; ignored when --bits/--hex are given
    #[arg(value_name = "DECIMAL", required_unless_present_any = ["bits", "hex"])]
    value: Option<String>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum FormatChoice {
    Fp16,
    Bfloat16,
    Fp32,
    Fp64,
    Tf32,
    Custom,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum RoundingMode {
    #[value(alias = "nearest", alias = "even")]
    HalfEven,
    #[value(alias = "trunc", alias = "zero")]
    TowardZero,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Notation {
    Plain,
    Scientific,
}

#[derive(Debug, Clone)]
struct FloatSpec {
    name: &'static str,
    exponent_bits: usize,
    significand_bits: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Class {
    Normal,
    Subnormal,
    Zero,
    PosInfinity,
    NegInfinity,
    Nan,
}

#[derive(Debug, Clone)]
struct SoftFloat {
    class: Class,
    sign: bool,
    exponent: i32,        // unbiased exponent for Normal/Subnormal; min exp for zero
    significand: BigUint, // stored fraction bits (no implicit leading 1)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let spec = resolve_format(&cli)?;

    let input_kind = if let Some(bits) = cli.bits.as_deref() {
        Input::Bits(bits.to_string())
    } else if let Some(hex) = cli.hex.as_deref() {
        Input::Hex(hex.to_string())
    } else {
        let raw = cli
            .value
            .clone()
            .expect("positional argument enforced by clap");
        Input::Decimal(raw)
    };

    let mut source_rational: Option<BigRational> = None;
    let soft = match input_kind {
        Input::Bits(b) => bits_to_softfloat(&b, &spec)?,
        Input::Hex(h) => {
            let bits = hex_to_bits(&h, total_bits(&spec)?)?;
            bits_to_softfloat(&bits, &spec)?
        }
        Input::Decimal(ref d) => {
            let parsed = parse_decimal(d)?;
            if let ParsedValue::Finite(ref v) = parsed {
                source_rational = Some(v.clone());
            }
            parsed_to_softfloat(&parsed, &spec, cli.rounding)
        }
    };

    let stored_value = softfloat_to_rational(&soft, &spec);
    let bits = softfloat_to_bits(&soft, &spec);
    let hex = bits_to_hex(&bits);

    println!("Format      : {}", spec.name);
    println!(
        "Layout      : 1 sign | {} exponent | {} significand",
        spec.exponent_bits, spec.significand_bits
    );
    println!("Class       : {:?}", soft.class);
    println!("Sign        : {}", if soft.sign { "-" } else { "+" });
    println!("Exponent    : {}", soft.exponent);
    println!("Binary      : {}", bits);
    println!("Hex         : {}", hex);

    if let Some(val) = stored_value {
        println!(
            "Stored      : {}",
            format_rational(&val, cli.precision, cli.notation)
        );
        if let Some(src) = source_rational {
            let err = &val - &src;
            println!(
                "Error       : {}",
                format_rational(&err, cli.precision, cli.notation)
            );
        }
    } else {
        println!("Stored      : {:?}", soft.class);
        if source_rational.is_some() {
            println!("Error       : (undefined for NaN/Infinity)");
        }
    }

    Ok(())
}

#[derive(Debug)]
enum Input {
    Decimal(String),
    Bits(String),
    Hex(String),
}

#[derive(Debug, Clone)]
enum ParsedValue {
    Finite(BigRational),
    PosInfinity,
    NegInfinity,
    Nan,
}

fn resolve_format(cli: &Cli) -> Result<FloatSpec> {
    let spec = match cli.format {
        FormatChoice::Fp16 => FloatSpec {
            name: "FP16",
            exponent_bits: 5,
            significand_bits: 10,
        },
        FormatChoice::Bfloat16 => FloatSpec {
            name: "bfloat16",
            exponent_bits: 8,
            significand_bits: 7,
        },
        FormatChoice::Fp32 => FloatSpec {
            name: "FP32",
            exponent_bits: 8,
            significand_bits: 23,
        },
        FormatChoice::Fp64 => FloatSpec {
            name: "FP64",
            exponent_bits: 11,
            significand_bits: 52,
        },
        FormatChoice::Tf32 => FloatSpec {
            name: "TensorFloat-32",
            exponent_bits: 8,
            significand_bits: 10,
        },
        FormatChoice::Custom => {
            let e = cli
                .exponent_bits
                .ok_or_else(|| anyhow!("--exp is required for --format=custom"))?;
            let s = cli
                .significand_bits
                .ok_or_else(|| anyhow!("--mant is required for --format=custom"))?;
            if !(2..=11).contains(&e) {
                bail!("exponent bits must be between 2 and 11");
            }
            if !(1..=52).contains(&s) {
                bail!("significand bits must be between 1 and 52");
            }
            FloatSpec {
                name: "Custom",
                exponent_bits: e,
                significand_bits: s,
            }
        }
    };
    Ok(spec)
}

fn total_bits(spec: &FloatSpec) -> Result<usize> {
    Ok(1 + spec.exponent_bits + spec.significand_bits)
}

fn parse_decimal(raw: &str) -> Result<ParsedValue> {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "inf" | "+inf" | "infinity" => Ok(ParsedValue::PosInfinity),
        "-inf" | "-infinity" => Ok(ParsedValue::NegInfinity),
        "nan" => Ok(ParsedValue::Nan),
        _ => {
            let dec = BigDecimal::from_str(raw)
                .with_context(|| format!("unable to parse decimal input: {raw}"))?;
            let (int, exp) = dec.into_bigint_and_exponent();
            let scale = if exp >= 0 {
                BigInt::one()
            } else {
                BigInt::from(10u32).pow((-exp) as u32)
            };
            let rat = BigRational::new(int, scale);
            Ok(ParsedValue::Finite(rat))
        }
    }
}

fn parsed_to_softfloat(value: &ParsedValue, spec: &FloatSpec, rounding: RoundingMode) -> SoftFloat {
    let sign = match value {
        ParsedValue::Finite(v) => v.is_negative(),
        ParsedValue::PosInfinity => false,
        ParsedValue::NegInfinity => true,
        ParsedValue::Nan => false,
    };

    match value {
        ParsedValue::Nan => SoftFloat {
            class: Class::Nan,
            sign: false,
            exponent: 0,
            significand: BigUint::zero(),
        },
        ParsedValue::PosInfinity => SoftFloat {
            class: Class::PosInfinity,
            sign: false,
            exponent: max_exponent(spec) + 1,
            significand: BigUint::zero(),
        },
        ParsedValue::NegInfinity => SoftFloat {
            class: Class::NegInfinity,
            sign: true,
            exponent: max_exponent(spec) + 1,
            significand: BigUint::zero(),
        },
        ParsedValue::Finite(v) if v.is_zero() => SoftFloat {
            class: Class::Zero,
            sign,
            exponent: min_exponent(spec),
            significand: BigUint::zero(),
        },
        ParsedValue::Finite(v) => {
            let abs = v.abs();
            let bias = bias(spec);
            let max_exp = bias as i32;
            let min_norm = 1 - bias as i32;

            let exp = log2_floor(&abs);

            if exp > max_exp {
                return SoftFloat {
                    class: if sign {
                        Class::NegInfinity
                    } else {
                        Class::PosInfinity
                    },
                    sign,
                    exponent: max_exp + 1,
                    significand: BigUint::zero(),
                };
            }

            if exp >= min_norm {
                quantize_normal(&abs, sign, exp, spec, rounding)
            } else {
                quantize_subnormal(&abs, sign, spec, rounding)
            }
        }
    }
}

fn bias(spec: &FloatSpec) -> i32 {
    (1i32 << (spec.exponent_bits - 1)) - 1
}

fn min_exponent(spec: &FloatSpec) -> i32 {
    1 - bias(spec)
}

fn log2_floor(r: &BigRational) -> i32 {
    let num_bits = r.numer().bits() as i32;
    let den_bits = r.denom().bits() as i32;
    let mut exp = num_bits - den_bits - 1;

    loop {
        let cmp_low = compare_pow2(r, exp);
        let cmp_high = compare_pow2(r, exp + 1);
        if (cmp_low != Ordering::Less) && cmp_high == Ordering::Less {
            return exp;
        }
        if cmp_low == Ordering::Less {
            exp -= 1;
        } else {
            exp += 1;
        }
    }
}

fn compare_pow2(r: &BigRational, exp: i32) -> Ordering {
    let pow = BigInt::one() << exp.abs();
    if exp >= 0 {
        r.numer().cmp(&(r.denom() * pow))
    } else {
        (r.numer() * pow).cmp(r.denom())
    }
}

fn quantize_normal(
    abs: &BigRational,
    sign: bool,
    exp: i32,
    spec: &FloatSpec,
    rounding: RoundingMode,
) -> SoftFloat {
    let frac = abs / pow2(exp);
    // frac should be in [1, 2)
    let mant = &frac - BigRational::one();
    let needed = spec.significand_bits + 3;
    let (bits, sticky) = fraction_bits(&mant, needed);
    let (mantissa, carry) = round_bits(bits, sticky, spec.significand_bits, rounding);

    let mut exponent = exp;
    let mut significand = mantissa;

    if carry {
        exponent += 1;
        significand = BigUint::zero();
    }

    let max_exp = bias(spec) as i32;
    if exponent > max_exp {
        return SoftFloat {
            class: if sign {
                Class::NegInfinity
            } else {
                Class::PosInfinity
            },
            sign,
            exponent,
            significand: BigUint::zero(),
        };
    }

    SoftFloat {
        class: Class::Normal,
        sign,
        exponent,
        significand,
    }
}

fn quantize_subnormal(
    abs: &BigRational,
    sign: bool,
    spec: &FloatSpec,
    rounding: RoundingMode,
) -> SoftFloat {
    let min_exp = min_exponent(spec);
    let scaled = abs / pow2(min_exp);
    let needed = spec.significand_bits + 3;
    let (bits, sticky) = fraction_bits(&scaled, needed);
    let (mantissa, carry) = round_bits(bits, sticky, spec.significand_bits, rounding);

    if carry {
        // Rounded up into the normal range at the smallest exponent.
        return SoftFloat {
            class: Class::Normal,
            sign,
            exponent: min_exp,
            significand: BigUint::zero(),
        };
    }

    let class = if mantissa.is_zero() {
        Class::Zero
    } else {
        Class::Subnormal
    };

    SoftFloat {
        class,
        sign,
        exponent: min_exp,
        significand: mantissa,
    }
}

fn pow2(exp: i32) -> BigRational {
    if exp >= 0 {
        BigRational::from_integer(BigInt::one() << exp)
    } else {
        BigRational::new(BigInt::one(), BigInt::one() << (-exp))
    }
}

fn fraction_bits(frac: &BigRational, bits: usize) -> (Vec<u8>, bool) {
    let mut result = Vec::with_capacity(bits);
    let mut remainder = frac.clone();
    let two = BigInt::from(2);
    for _ in 0..bits {
        let doubled = &remainder * &two;
        if doubled >= BigRational::one() {
            result.push(1);
            remainder = doubled - BigRational::one();
        } else {
            result.push(0);
            remainder = doubled;
        }
    }
    let sticky = !remainder.is_zero();
    (result, sticky)
}

fn round_bits(bits: Vec<u8>, sticky: bool, width: usize, mode: RoundingMode) -> (BigUint, bool) {
    let kept = &bits[..width];
    let kept_value = bits_to_uint(kept);

    match mode {
        RoundingMode::TowardZero => (kept_value, false),
        RoundingMode::HalfEven => {
            if width >= bits.len() {
                return (kept_value, false);
            }
            let guard = bits.get(width).copied().unwrap_or(0);
            let round_bit = bits.get(width + 1).copied().unwrap_or(0);
            let rest_sticky = sticky || bits.iter().skip(width + 2).any(|b| *b == 1);

            let should_increment = match (guard, round_bit, rest_sticky) {
                (1, 0, false) => kept.last().copied().unwrap_or(0) == 1,
                (1, _, _) => true,
                _ => false,
            };

            if should_increment {
                let max_val = (BigUint::one() << width) - BigUint::one();
                if kept_value == max_val {
                    (BigUint::zero(), true)
                } else {
                    (kept_value + BigUint::one(), false)
                }
            } else {
                (kept_value, false)
            }
        }
    }
}

fn bits_to_uint(bits: &[u8]) -> BigUint {
    let mut value = BigUint::zero();
    for &b in bits {
        value <<= 1;
        if b == 1 {
            value += 1u8;
        }
    }
    value
}

fn softfloat_to_bits(sf: &SoftFloat, spec: &FloatSpec) -> String {
    let mut out = String::with_capacity(total_bits(spec).unwrap_or(0));
    out.push(if sf.sign { '1' } else { '0' });

    let exp_bits = spec.exponent_bits;
    let frac_bits = spec.significand_bits;

    match sf.class {
        Class::PosInfinity | Class::NegInfinity => {
            out.push_str(&"1".repeat(exp_bits));
            out.push_str(&"0".repeat(frac_bits));
        }
        Class::Nan => {
            out.push_str(&"1".repeat(exp_bits));
            out.push_str(&"1".repeat(frac_bits));
        }
        Class::Zero | Class::Subnormal => {
            out.push_str(&"0".repeat(exp_bits));
            out.push_str(&format!("{:0width$b}", sf.significand, width = frac_bits));
        }
        Class::Normal => {
            let biased = (sf.exponent + bias(spec)) as u64;
            out.push_str(&format!("{:0width$b}", biased, width = exp_bits));
            out.push_str(&format!("{:0width$b}", sf.significand, width = frac_bits));
        }
    }

    out
}

fn bits_to_hex(bits: &str) -> String {
    let padded_len = ((bits.len() + 3) / 4) * 4;
    let mut padded = bits.to_string();
    while padded.len() < padded_len {
        padded.insert(0, '0');
    }

    padded
        .as_bytes()
        .chunks(4)
        .map(|chunk| {
            let s = std::str::from_utf8(chunk).unwrap();
            let v = u8::from_str_radix(s, 2).unwrap();
            format!("{:x}", v)
        })
        .collect::<String>()
        .trim_start_matches('0')
        .to_string()
        .to_uppercase()
}

fn hex_to_bits(hex: &str, total_bits: usize) -> Result<String> {
    let cleaned = hex.trim().trim_start_matches("0x").trim_start_matches("0X");
    let bits_needed = total_bits;
    let expected_hex = (bits_needed + 3) / 4;
    let mut padded = cleaned.to_string();
    if padded.len() < expected_hex {
        padded = "0".repeat(expected_hex - padded.len()) + &padded;
    }
    if padded.len() != expected_hex {
        bail!(
            "hex length ({}) does not match expected bits {}",
            padded.len(),
            bits_needed
        );
    }
    let mut bits = String::with_capacity(bits_needed);
    for ch in padded.chars() {
        let val = ch
            .to_digit(16)
            .ok_or_else(|| anyhow!("invalid hex digit: {ch}"))?;
        bits.push_str(&format!("{:04b}", val));
    }
    if bits.len() > bits_needed {
        bits = bits[bits.len() - bits_needed..].to_string();
    }
    Ok(bits)
}

fn bits_to_softfloat(bits: &str, spec: &FloatSpec) -> Result<SoftFloat> {
    let cleaned = bits
        .trim()
        .strip_prefix("0b")
        .or_else(|| bits.trim().strip_prefix("0B"))
        .unwrap_or_else(|| bits.trim());
    let total = total_bits(spec)?;
    if cleaned.len() != total {
        bail!("expected {} bits, got {}", total, cleaned.len());
    }
    if !cleaned.chars().all(|c| c == '0' || c == '1') {
        bail!("bits must contain only 0 or 1");
    }

    let sign = cleaned.as_bytes()[0] == b'1';
    let exp_bits = &cleaned[1..1 + spec.exponent_bits];
    let frac_bits = &cleaned[1 + spec.exponent_bits..];

    let exp_val = usize::from_str_radix(exp_bits, 2)?;
    let mantissa = BigUint::parse_bytes(frac_bits.as_bytes(), 2)
        .ok_or_else(|| anyhow!("invalid mantissa bits"))?;

    let all_exp_ones = exp_bits.chars().all(|c| c == '1');
    let all_exp_zero = exp_bits.chars().all(|c| c == '0');
    let all_frac_zero = mantissa.is_zero();

    let bias = bias(spec);
    let min_exp = min_exponent(spec);

    let class;
    let exponent;

    if all_exp_ones {
        class = if all_frac_zero {
            if sign {
                Class::NegInfinity
            } else {
                Class::PosInfinity
            }
        } else {
            Class::Nan
        };
        exponent = max_exponent(spec);
    } else if all_exp_zero {
        class = if all_frac_zero {
            Class::Zero
        } else {
            Class::Subnormal
        };
        exponent = min_exp;
    } else {
        class = Class::Normal;
        exponent = exp_val as i32 - bias;
    }

    Ok(SoftFloat {
        class,
        sign,
        exponent,
        significand: mantissa,
    })
}

fn max_exponent(spec: &FloatSpec) -> i32 {
    bias(spec)
}

fn softfloat_to_rational(sf: &SoftFloat, spec: &FloatSpec) -> Option<BigRational> {
    match sf.class {
        Class::PosInfinity | Class::NegInfinity | Class::Nan => None,
        Class::Zero => Some(BigRational::zero()),
        Class::Subnormal => {
            let denom = BigInt::one() << spec.significand_bits;
            let sig = BigRational::new(
                sf.significand.to_bigint().unwrap_or_else(BigInt::zero),
                denom,
            );
            let value = sig * pow2(min_exponent(spec));
            Some(if sf.sign { -value } else { value })
        }
        Class::Normal => {
            let denom = BigInt::one() << spec.significand_bits;
            let leading = BigRational::one();
            let frac = BigRational::new(
                sf.significand.to_bigint().unwrap_or_else(BigInt::zero),
                denom,
            );
            let sig = leading + frac;
            let value = sig * pow2(sf.exponent);
            Some(if sf.sign { -value } else { value })
        }
    }
}

fn format_rational(value: &BigRational, precision: usize, notation: Notation) -> String {
    if value.is_zero() {
        return "0".to_string();
    }

    let sign = value.is_negative();
    let abs = value.abs();
    let integer = (abs.numer() / abs.denom())
        .to_bigint()
        .unwrap_or_else(BigInt::zero);
    let mut remainder = abs - BigRational::from_integer(integer.clone());

    let mut digits = String::new();
    for _ in 0..precision {
        remainder *= BigInt::from(10);
        let digit = (remainder.numer() / remainder.denom())
            .to_bigint()
            .unwrap_or_else(BigInt::zero);
        digits.push_str(&format!("{}", digit));
        remainder -= BigRational::from_integer(digit);
        if remainder.is_zero() {
            break;
        }
    }

    let mut repr = if digits.is_empty() {
        format!("{}", integer)
    } else {
        format!("{}.{digits}", integer)
    };

    if let Notation::Scientific = notation {
        repr = to_scientific(&repr);
    }

    if sign { format!("-{repr}") } else { repr }
}

fn to_scientific(num: &str) -> String {
    if num == "0" {
        return "0".to_string();
    }
    let mut cleaned = num.replace('.', "");
    let mut exponent = 0i32;
    let negative = cleaned.starts_with('-');
    if negative {
        cleaned.remove(0);
    }
    let mut chars: Vec<char> = cleaned.chars().collect();
    while !chars.is_empty() && chars[0] == '0' {
        chars.remove(0);
        exponent -= 1;
    }
    let first = chars.get(0).cloned().unwrap_or('0');
    let rest: String = chars.iter().skip(1).collect();
    let mantissa = if rest.is_empty() {
        format!("{first}")
    } else {
        format!("{first}.{}", rest.trim_end_matches('0'))
    };
    let exp_str = format!(
        "e{:+}",
        exponent + ((num.find('.').unwrap_or(num.len()) as i32) - 1)
    );
    let sign_prefix = if negative { "-" } else { "" };
    format!("{sign_prefix}{mantissa}{exp_str}")
}
#[cfg(test)]
mod tests;
