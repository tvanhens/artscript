import hashlib
import json
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
NAMES = ['review-six', 'plan-six', 'simplify-six', 'relationships-six', 'contours-six', 'medium-six', 'contours-medium-six', 'contours-review-six', 'contours-plan-six', 'contours-holdout', 'medium-holdout']


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + '\n')


def portable(value):
    if isinstance(value, dict):
        return {k: portable(v) for k, v in value.items()}
    if isinstance(value, list):
        return [portable(v) for v in value]
    if isinstance(value, str):
        return value.replace(str(REPO) + '/', '').replace('/Users/tvanhens/', '<historical-home>/')
    return value


def main():
    from PIL import Image, ImageDraw, ImageOps
    captures = REPO / 'bench/out/paired'
    runs = []
    for name in NAMES:
        report_path = captures / name / 'report.json'
        report = json.loads(report_path.read_text())
        sources = [Path(s['path']).parent for s in report.get('sources', [])] or [report_path.parent]
        lookup = {}
        manifests = []
        for source in sources:
            exp_path = source / 'experiment.json'
            exp = json.loads(exp_path.read_text())
            manifests.append({'capture': source.name, 'sha256': sha(exp_path), 'experiment': portable(exp)})
            for arm in ['baseline', 'candidate']:
                for f in (source / 'inputs' / arm).rglob('*'):
                    if f.is_file():
                        target = ROOT / 'skills' / sha(f) / f.name
                        target.parent.mkdir(parents=True, exist_ok=True)
                        shutil.copyfile(f, target)
            for scheduled in exp['schedule']:
                lookup[scheduled['prompt_id'], scheduled['repetition']] = source / 'pairs' / scheduled['directory']
        rows = []
        pictures = {}
        for row in report['pairs']:
            pair = lookup[row['prompt_id'], row['repetition']]
            item = portable(row)
            item['capture'] = str(pair.relative_to(captures))
            item['pair_receipt_sha256'] = sha(pair / 'pair.json')
            item['judge_outputs'] = {f.name: {'sha256': sha(f), 'value': json.loads(f.read_text())} for f in sorted(pair.glob('judge-*-output.json'))}
            for arm in ['baseline', 'candidate']:
                generation = item['generations'][arm]
                generation['artifact_sha256'] = {f: sha(pair / arm / f) for f in ['original.png', 'rebuilt.png'] if (pair / arm / f).exists()}
                generation['source_sha256'] = {str(f.relative_to(pair / arm / 'project')): sha(f) for f in sorted((pair / arm / 'project').rglob('*')) if f.is_file() and (f.suffix == '.ramo' or f.name == 'ramose.toml')}
                if name in ['contours-six', 'contours-holdout']:
                    target = ROOT / 'art' / name / f"{row['prompt_id']}-{row['repetition']}" / arm
                    for rel in generation['source_sha256']:
                        dst = target / rel
                        dst.parent.mkdir(parents=True, exist_ok=True)
                        shutil.copyfile(pair / arm / 'project' / rel, dst)
                    generation['archived_source'] = str(target.relative_to(ROOT))
                    pictures.setdefault(row['prompt_id'], []).append((row['repetition'], arm, pair / arm / 'original.png'))
            rows.append(item)
        for prompt, cells in pictures.items():
            sheet = Image.new('RGB', (640, 5 * 350), '#eeeeee')
            draw = ImageDraw.Draw(sheet)
            for repetition, arm, path in cells:
                image = Image.open(path).convert('RGBA')
                thumb = ImageOps.contain(image, (312, 320))
                x = 0 if arm == 'baseline' else 320
                y = repetition * 350
                sheet.paste(thumb, (x + (320 - thumb.width) // 2, y + 24 + (320 - thumb.height) // 2), thumb)
                draw.text((x + 8, y + 6), f'{prompt} | repetition {repetition} | {arm}', fill='black')
            target = ROOT / 'sheets' / f'{name}-{prompt}.jpg'
            target.parent.mkdir(exist_ok=True)
            sheet.save(target, quality=88)
        runs.append({'name': name, 'source_report_sha256': sha(report_path), 'original_summary': report['summary'], 'original_per_prompt': report['per_prompt'], 'manifests': manifests, 'pairs': rows})
    write(ROOT / 'evidence.json', {'schema': 'ramose-hillclimb-history-v1', 'longitudinal_eligible': False, 'runs': runs})
    frozen = ROOT / 'frozen-evaluator'
    frozen.mkdir(exist_ok=True)
    for file in ['paired.py', 'wow.py', 'combine_paired.py', 'provenance.json']:
        shutil.copyfile(REPO / '.audit/hillclimb/frozen-evaluator' / file, frozen / file)
    write(ROOT / 'inventory.json', {str(f.relative_to(ROOT)): sha(f) for f in sorted(ROOT.rglob('*')) if f.is_file() and f.name not in ['inventory.json', 'analysis.json', 'README.md', 'analyze.py', 'export.py'] and '__pycache__' not in str(f)})


if __name__ == '__main__':
    main()
