"""Fit each spine's type placement to the reference photo by coordinate descent.

Every spine line in art/narnia.art is generated from tools/spines.json. Each round perturbs one
parameter on every spine at once, rebuilds, and keeps the step per spine only where that
spine's mean OKLab ΔE inside its fixed face box drops. Prints the score per round.
"""
import json, subprocess, sys, re
import numpy as np
np.seterr(all="ignore")
from PIL import Image, ImageFilter

ROOT = __file__.rsplit('/tools/', 1)[0]
ART = f'{ROOT}/art/narnia.art'
SPEC = f'{ROOT}/tools/spines.json'
BIN = f'{ROOT}/../../target/release/artscript'

def oklab(img):
    a = np.asarray(img.filter(ImageFilter.GaussianBlur(1.2)).convert('RGB')).astype(np.float64) / 255
    a = np.where(a <= .04045, a / 12.92, ((a + .055) / 1.055) ** 2.4)
    M1 = np.array([[.4122214708, .5363325363, .0514459929], [.2119034982, .6806995451, .1073969566], [.0883024619, .2817188376, .6299787005]])
    M2 = np.array([[.2104542553, .7936177850, -.0040720468], [1.9779984951, -2.4285922050, .4505937099], [.0259040371, .7827717662, -.8086757660]])
    return np.cbrt(a @ M1.T) @ M2.T

REF = oklab(Image.open(f'{ROOT}/ref/narnia.jpg'))

def line(name, b):
    keys = ' '.join(f'{k}:{v}' for k, v in b['fixed'].items())
    f = b['fit']
    return (f"  spine {b['w']} {keys} title:{f['tx']},{f['ty']} lewis:{f['lx']},{f['ly']} "
            f"tsize:{f['ts']} lsize:{f['ls']} label:{f['lb']} at:{b['x']},0 name:{name}")

def write(spec):
    src = open(ART).read()
    body = '\n'.join(line(n, b) for n, b in spec.items())
    src = re.sub(r'(  spine .*\n)+', lambda m: body + '\n', src, count=1)
    open(ART, 'w').write(src)

def scores(spec):
    write(spec)
    subprocess.run([BIN, 'build', '--only', 'narnia.png'], cwd=ROOT, check=True, capture_output=True)
    out = oklab(Image.open(f'{ROOT}/assets/narnia.png'))
    de = np.linalg.norm(out - REF, axis=2) * 100
    res = {}
    for n, b in spec.items():
        x0 = int(b['x'] - b['w'] / 2) + 2
        top = int(b['fixed']['top'])
        res[n] = de[top + 4:853, x0:x0 + b['w'] - 4].mean()
    return res, de.mean()

STEPS = {'tx': 1, 'ty': 3, 'lx': 1, 'ly': 3, 'ts': .5, 'ls': .5, 'lb': 2}

def main(rounds):
    spec = json.load(open(SPEC))
    best, total = scores(spec)
    print(f'start  mean ΔE {total:.2f}  ' + ' '.join(f'{n}:{v:.2f}' for n, v in best.items()), flush=True)
    for r in range(rounds):
        for k, step in STEPS.items():
            for sign in (1, -1):
                trial = json.loads(json.dumps(spec))
                for b in trial.values():
                    b['fit'][k] = round(b['fit'][k] + sign * step, 2)
                got, _ = scores(trial)
                for n in spec:
                    if got[n] < best[n] - 1e-3:
                        spec[n]['fit'][k] = trial[n]['fit'][k]
                        best[n] = got[n]
        json.dump(spec, open(SPEC, 'w'), indent=1)
        _, total = scores(spec)
        print(f'round {r + 1}  mean ΔE {total:.2f}  ' + ' '.join(f'{n}:{v:.2f}' for n, v in best.items()), flush=True)

if __name__ == '__main__':
    main(int(sys.argv[1]) if len(sys.argv) > 1 else 3)
