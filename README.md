# afcvt

[简体中文版本](README.zh.md)

A command-line utility that converts between decimal numbers and IEEE754-style floating-point bit layouts. Presets cover FP16, bfloat16, FP32, FP64, and TF32; custom formats are supported via exponent and significand widths.

## Build
- From repo root: `cargo build --release`
- Binary output: `target/release/afcvt`

## Usage
- Help: `afcvt --help`
- Convert with FP32 (default): `afcvt 1.5`
- Choose preset: `afcvt --format fp64 0.1`
- Custom format: `afcvt --format custom --exp 8 --mant 23 1.0`
- Raw bits: `afcvt --format fp32 --bits 00111111110000000000000000000000`
- Hex bits: `afcvt --format fp32 --hex 0x3fc00000`

## Notes
- When `--bits` or `--hex` is set, the positional decimal input is ignored.
- `--exp` and `--mant` apply only to `--format custom`, specifying exponent and significand widths.
