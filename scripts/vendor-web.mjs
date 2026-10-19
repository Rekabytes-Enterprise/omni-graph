// Refresh the pinned offline browser dependencies; commit the generated files.
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../', import.meta.url));
const source = root + 'node_modules/three/';
const target = root + 'src/web/vendor/';
mkdirSync(target, { recursive: true });
for (const file of ['three.module.js', 'three.core.js']) copyFileSync(source + 'build/' + file, target + file);
const orbit = readFileSync(source + 'examples/jsm/controls/OrbitControls.js', 'utf8');
if (!orbit.includes("from 'three'")) throw new Error('Review updated OrbitControls imports before vendoring');
writeFileSync(target + 'OrbitControls.js', orbit.replaceAll("from 'three'", "from './three.module.js'"));
copyFileSync(source + 'LICENSE', target + 'THREE-LICENSE.txt');
console.log('Vendored Three.js ' + JSON.parse(readFileSync(source + 'package.json')).version + ' and OrbitControls; only the bare module import was changed.');
