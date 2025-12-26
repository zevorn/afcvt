"use strict";

const vm = require("node:vm");

const ASSET_MANIFEST_URL = "https://flop.evanau.dev/asset-manifest.json";
const TSC_URLS = [
	"https://unpkg.com/typescript@5.6.3/lib/typescript.js",
	"https://cdn.jsdelivr.net/npm/typescript@5.6.3/lib/typescript.js",
];
const BIGNUM_URLS = [
	"https://cdn.jsdelivr.net/npm/bignumber.js@9.1.2/bignumber.js",
	"https://unpkg.com/bignumber.js@9.1.2/bignumber.js",
];

const parseArgs = () => {
	const args = process.argv.slice(2);
	const result = { format: "FP16", limit: null, exp: null, mant: null };
	for (const arg of args) {
		if (arg.startsWith("--format=")) {
			result.format = arg.replace("--format=", "");
		} else if (arg.startsWith("--limit=")) {
			result.limit = Number(arg.replace("--limit=", ""));
		} else if (arg.startsWith("--exp=")) {
			result.exp = Number(arg.replace("--exp=", ""));
		} else if (arg.startsWith("--mant=")) {
			result.mant = Number(arg.replace("--mant=", ""));
		}
	}
	return result;
};

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const fetchJson = async (url, attempts = 3) => {
	let lastErr = null;
	for (let i = 0; i < attempts; i++) {
		try {
			const res = await fetch(url);
			if (!res.ok) throw new Error(`failed to fetch ${url}: ${res.status}`);
			return await res.json();
		} catch (err) {
			lastErr = err;
			await sleep(200 * (i + 1));
		}
	}
	throw lastErr;
};

const fetchText = async (url, attempts = 3) => {
	let lastErr = null;
	for (let i = 0; i < attempts; i++) {
		try {
			const res = await fetch(url);
			if (!res.ok) throw new Error(`failed to fetch ${url}: ${res.status}`);
			return await res.text();
		} catch (err) {
			lastErr = err;
			await sleep(200 * (i + 1));
		}
	}
	throw lastErr;
};

const fetchTextWithFallback = async (urls) => {
	let lastErr = null;
	for (const url of urls) {
		try {
			return await fetchText(url);
		} catch (err) {
			lastErr = err;
		}
	}
	if (lastErr) throw lastErr;
	throw new Error("all fetch attempts failed");
};

const loadTypeScript = async () => {
	const code = await fetchTextWithFallback(TSC_URLS);
	const ctx = { module: {}, exports: {} };
	vm.runInNewContext(code, ctx);
	return ctx.ts || ctx.module.exports;
};

const loadBigNumber = async () => {
	const code = await fetchTextWithFallback(BIGNUM_URLS);
	const ctx = { module: { exports: {} }, exports: {}, self: {}, window: {} };
	vm.runInNewContext(code, ctx);
	return ctx.module.exports;
};

const transpileModule = (ts, code, filename) => {
	return ts.transpileModule(code, {
		compilerOptions: {
			module: ts.ModuleKind.CommonJS,
			target: ts.ScriptTarget.ES2020,
			esModuleInterop: true,
		},
		fileName: filename,
	}).outputText;
};

const evalModule = (jsCode, requireMap, filename) => {
	const module = { exports: {} };
	const sandbox = {
		module,
		exports: module.exports,
		require: (name) => {
			if (!requireMap[name]) {
				throw new Error(`unknown require ${name}`);
			}
			return requireMap[name];
		},
		process: { env: {} },
		console,
	};
	vm.runInNewContext(jsCode, sandbox, { filename });
	return module.exports;
};

const extractSources = (mapJson, name) => {
	const idx = mapJson.sources.indexOf(name);
	if (idx === -1) throw new Error(`source ${name} not found`);
	return mapJson.sourcesContent[idx];
};

const buildReference = async () => {
	const { format, limit, exp, mant } = parseArgs();
	const manifest = await fetchJson(ASSET_MANIFEST_URL);
	const mapPath = manifest.files["main.js.map"];
	if (!mapPath) throw new Error("missing main.js.map");
	const mapJson = await fetchJson(`https://flop.evanau.dev${mapPath}`);

	const ts = await loadTypeScript();
	const BigNumber = await loadBigNumber();

	const constantsTs = extractSources(mapJson, "constants.ts");
	const flopTs = extractSources(mapJson, "converter/flop.ts");

	const constantsJs = transpileModule(ts, constantsTs, "constants.ts");
	const flopJs = transpileModule(ts, flopTs, "flop.ts");

	const constants = evalModule(constantsJs, {}, "constants.js");
	const flop = evalModule(
		flopJs,
		{
			"../constants": constants,
			"bignumber.js": BigNumber,
		},
		"flop.js"
	);

	BigNumber.set({ DECIMAL_PLACES: 4096 });

	const formats = {
		FP16: { exponentWidth: constants.FP16.exponentWidth, significandWidth: constants.FP16.significandWidth },
		BF16: { exponentWidth: constants.BF16.exponentWidth, significandWidth: constants.BF16.significandWidth },
		TF32: { exponentWidth: constants.TF32.exponentWidth, significandWidth: constants.TF32.significandWidth },
		FP32: { exponentWidth: constants.FP32.exponentWidth, significandWidth: constants.FP32.significandWidth },
		FP64: { exponentWidth: constants.FP64.exponentWidth, significandWidth: constants.FP64.significandWidth },
	};
	let target = formats[format];
	if (!target) {
		if (exp && mant) {
			target = { exponentWidth: exp, significandWidth: mant };
		} else {
			throw new Error(`unsupported format ${format}; provide --exp and --mant`);
		}
	}

	const totalBits = 1 + target.exponentWidth + target.significandWidth;
	const hexDigits = Math.ceil(totalBits / 4);
	const max = limit ?? (totalBits <= 16 ? Math.pow(2, totalBits) : 4096);

	const samples = [];
	for (let i = 0; i < max; i++) {
		const hex = i.toString(16).padStart(hexDigits, "0");
		const bits = flop.bitsFromHexString(hex, totalBits);
		const flop754 = flop.generateFlop754(
			bits.slice(0, 1),
			bits.slice(1, 1 + target.exponentWidth),
			bits.slice(1 + target.exponentWidth)
		);
		const flopVal = flop.convertFlop754ToFlop(flop754);

		let fraction = null;
		if (flopVal.type === flop.FlopType.NORMAL) {
			const [num, den] = flopVal.value.toFraction();
			fraction = { num: num.toFixed(), den: den.toFixed() };
		}

		samples.push({
			hex,
			bits: flop.stringifyBits(bits),
			type: flop754.type,
			sign: flop754.sign,
			exponent: flop754.exponent,
			significand: flop754.significand.toString(),
			fraction,
		});
	}

	const result = {
		format,
		exponentWidth: target.exponentWidth,
		significandWidth: target.significandWidth,
		totalBits,
		count: samples.length,
		samples,
	};

	process.stdout.write(JSON.stringify(result));
};

buildReference().catch((err) => {
	console.error(err);
	process.exit(1);
});
