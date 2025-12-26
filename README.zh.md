# afcvt

一个在十进制数与 IEEE754 风格浮点二进制表示之间互转的命令行工具，内置 FP16、bfloat16、FP32、FP64、TF32，并支持指定指数位和尾数位的自定义格式。

## 构建
- 在仓库根目录执行：`cargo build --release`
- 可执行文件在 `target/release/afcvt`

## 使用
- 查看帮助：`afcvt --help`
- 默认 FP32：`afcvt 1.5`
- 选择预设：`afcvt --format fp64 0.1`
- 自定义格式：`afcvt --format custom --exp 8 --mant 23 1.0`
- 直接输入比特串：`afcvt --format fp32 --bits 00111111110000000000000000000000`
- 直接输入十六进制：`afcvt --format fp32 --hex 0x3fc00000`

## 说明
- 提供 `--bits` 或 `--hex` 时会忽略位置参数的十进制输入。
- `--exp` 与 `--mant` 仅适用于 `--format custom`，分别表示指数位数与尾数位数。
