#!/usr/bin/env node

const fs = require('node:fs');
const path = require('node:path');
const ts = require('typescript');

const packageDir = path.resolve(__dirname, '..');
const inputFile = path.join(packageDir, 'src', 'index.ts');
const outputFile = path.join(packageDir, 'dist', 'index.cjs');

const source = fs.readFileSync(inputFile, 'utf8');
const result = ts.transpileModule(source, {
  compilerOptions: {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.CommonJS,
    moduleResolution: ts.ModuleResolutionKind.NodeJs,
    esModuleInterop: true,
    sourceMap: false,
    declaration: false,
  },
  fileName: inputFile,
  reportDiagnostics: true,
});

const diagnostics = result.diagnostics?.filter(
  (diagnostic) => diagnostic.category === ts.DiagnosticCategory.Error,
);

if (diagnostics && diagnostics.length > 0) {
  const formatted = ts.formatDiagnosticsWithColorAndContext(diagnostics, {
    getCanonicalFileName: (fileName) => fileName,
    getCurrentDirectory: () => packageDir,
    getNewLine: () => '\n',
  });
  console.error(formatted);
  process.exit(1);
}

fs.mkdirSync(path.dirname(outputFile), { recursive: true });
fs.writeFileSync(outputFile, result.outputText, 'utf8');
