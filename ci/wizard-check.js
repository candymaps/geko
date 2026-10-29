// The manifest of the operator that the wizard of the docs writes (jekyll/assets/js/geko-wizard.js) is
// what `geko k8s install` writes. With the defaults it is deploy/operator/resources.yaml, which
// ci/deploy.sh keeps equal to the code; with other options, ci/deploy.sh gives the output of geko for
// the same ones: `node ci/wizard-check.js <geko's output> [--tag T --storage S ...]`.
"use strict";
const fs = require("node:fs");
const path = require("node:path");
const wizard = require("../jekyll/assets/js/geko-wizard.js");

const [file, ...rest] = process.argv.slice(2);
const names = { "--tag": "tag", "--storage": "storage", "--storage-class": "storageClass", "--access-mode": "accessMode", "--host": "host", "--image-prefix": "imagePrefix", "--operator-image": "operatorImage" };
const options = {};
for (let i = 0; i < rest.length; i += 2) {
  if (!names[rest[i]]) throw new Error(`unknown option ${rest[i]}`);
  options[names[rest[i]]] = rest[i + 1];
}

const expected = fs.readFileSync(path.resolve(file), "utf8");
const written = wizard.install(options);
if (written !== expected) {
  const a = expected.split("\n");
  const b = written.split("\n");
  const line = a.findIndex((l, i) => l !== b[i]);
  console.error(`the wizard does not write what geko k8s install writes (${file}), from line ${line + 1}:`);
  console.error(`  geko:   ${a[line]}`);
  console.error(`  wizard: ${b[line]}`);
  process.exit(1);
}
console.log(`the wizard writes ${file}${rest.length ? ` with ${rest.join(" ")}` : ""}`);
