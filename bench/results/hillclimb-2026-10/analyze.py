"""Verify archived receipts and reproduce historical and prompt-cluster estimates."""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import random
import statistics

ROOT = Path(__file__).resolve().parent
ARMS = ('baseline', 'candidate')
AXES = ('fidelity', 'composition', 'color', 'form', 'polish', 'wow')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def historical_summarizer():
    tree = ast.parse((ROOT / 'frozen-evaluator/paired.py').read_text())
    node = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'summarize')
    scope = {'statistics': statistics, 'random': random, 'ARMS': ARMS}
    exec(compile(ast.Module(body=[node], type_ignores=[]), '<frozen summarize>', 'exec'), scope)
    return scope['summarize']


def verify_judgment(row):
    outputs = row['judge_outputs']
    for output in outputs.values():
        value = output['value']
        ratings = [value['left'], value['right']] if 'left' in value else [value]
        for rating in ratings:
            require(all(type(rating.get(axis)) is int and 1 <= rating[axis] <= 20 for axis in AXES), 'invalid judge axis score')
            require(isinstance(rating.get('notes'), str), 'missing judge notes')
    successful = [arm for arm in ARMS if row['generations'][arm]['status'] == 'success']
    scores = dict.fromkeys(ARMS, 0.0)
    preferences = []
    if len(successful) == 2:
        observations = {arm: [] for arm in ARMS}
        for index, order in enumerate((ARMS, ARMS[::-1])):
            matches = [value['value'] for name, value in outputs.items() if name.startswith(f'judge-{index}-')]
            require(len(matches) == 1, 'expected one retained successful judge output per orientation')
            result = matches[0]
            for side, arm in zip(('left', 'right'), order):
                observations[arm].append(statistics.mean(result[side][axis] for axis in AXES))
            preferences.append('tie' if result['preference'] == 'tie' else order[('left', 'right').index(result['preference'])])
        scores = {arm: statistics.mean(observations[arm]) for arm in ARMS}
    elif successful:
        require(len(outputs) == 1, 'expected one single-image judgment')
        result = next(iter(outputs.values()))['value']
        scores[successful[0]] = statistics.mean(result[axis] for axis in AXES)
        preferences = successful
    else:
        preferences = ['tie']
    require(scores == row['judgment']['scores'], f"judge score mismatch at {row['capture']}")
    require(preferences == row['judgment']['preferences'], 'preference mismatch')
    for generation in row['generations'].values():
        if 'archived_source' in generation:
            for rel, expected in generation['source_sha256'].items():
                actual = hashlib.sha256((ROOT / generation['archived_source'] / rel).read_bytes()).hexdigest()
                require(actual == expected, f'source changed at {rel}')


def analyze():
    inventory = json.loads((ROOT / 'inventory.json').read_text())
    included = {str(f.relative_to(ROOT)) for f in ROOT.rglob('*') if f.is_file() and f.name not in ['inventory.json', 'analysis.json', 'README.md', 'analyze.py', 'export.py'] and '__pycache__' not in str(f)}
    require(set(inventory) == included, 'archive inventory has missing or unexpected entries')
    for rel, expected in inventory.items():
        require(hashlib.sha256((ROOT / rel).read_bytes()).hexdigest() == expected, f'archive hash mismatch at {rel}')
    frozen = json.loads((ROOT / 'frozen-evaluator/provenance.json').read_text())
    for name, expected in frozen.items():
        require(hashlib.sha256((ROOT / 'frozen-evaluator' / name).read_bytes()).hexdigest() == expected, 'frozen evaluator provenance mismatch')
    evidence = json.loads((ROOT / 'evidence.json').read_text())
    summarize = historical_summarizer()
    results = []
    for run in evidence['runs']:
        rows = run['pairs']
        prompts = {}
        for manifest in run['manifests']:
            exp = manifest['experiment']
            require(exp['harness_sha256'] == frozen['paired.py'], 'historical evaluator mismatch')
            for prompt, text in exp['prompts']:
                require(prompt not in prompts, 'duplicate prompt across manifests')
                prompts[prompt] = text
                schedule = [s['repetition'] for s in exp['schedule'] if s['prompt_id'] == prompt]
                require(sorted(schedule) == list(range(exp['repetitions'])), 'incomplete repetition schedule')
            for arm in ARMS:
                for name, expected_hash in exp['provenance'][arm].items():
                    snapshot = ROOT / 'skills' / expected_hash / Path(name).name
                    require(hashlib.sha256(snapshot.read_bytes()).hexdigest() == expected_hash, 'skill snapshot mismatch')
        identities = [(row['prompt_id'], row['repetition']) for row in rows]
        expected = [(p['prompt_id'], p['repetition']) for m in run['manifests'] for p in m['experiment']['schedule']]
        require(len(set(identities)) == len(identities) and sorted(identities) == sorted(expected), 'schedule mismatch')
        for row in rows:
            require(row['prompt'] == prompts[row['prompt_id']], 'pair prompt text differs from manifest')
            verify_judgment(row)
        original = summarize(rows, len(expected))
        require(original == run['original_summary'], f"historical summary mismatch for {run['name']}")
        prompt_means = {}
        for prompt in dict.fromkeys(row['prompt_id'] for row in rows):
            subset = [row for row in rows if row['prompt_id'] == prompt]
            summary = summarize(subset, len(subset))
            require(summary == run['original_per_prompt'][prompt], f'per-prompt summary mismatch at {prompt}')
            prompt_means[prompt] = summary['paired_delta_mean']
        rng = random.Random(0)
        means = list(prompt_means.values())
        boot = sorted(statistics.mean(rng.choices(means, k=len(means))) for _ in range(4000))
        delivered = [row for row in rows if all(row['generations'][a]['status'] == 'success' for a in ARMS)]
        results.append({'name': run['name'], 'pairs': len(rows), 'prompts': len(means),
                        'baseline_mean': original['arms']['baseline']['mean'],
                        'candidate_mean': original['arms']['candidate']['mean'],
                        'delta': original['paired_delta_mean'],
                        'historical_pair_ci95': original['paired_delta_ci95'],
                        'retrospective_prompt_cluster_ci95': [boot[100], boot[3899]],
                        'per_prompt_delta': prompt_means,
                        'generation_failures': sum(original['arms'][a]['failures'] for a in ARMS),
                        'both_delivered_pairs': len(delivered),
                        'both_delivered_delta': statistics.mean(r['judgment']['scores']['candidate'] - r['judgment']['scores']['baseline'] for r in delivered)})
    require(sum(r['pairs'] for r in results) == 300, 'expected 300 pairs')
    require(sum(r['generation_failures'] for r in results) == 1, 'expected one delivery failure')
    return {'method': 'Original frozen pair bootstrap and retrospective prompt-cluster percentile bootstrap. Both use 4000 draws, seed 0, order-statistic indices 100 and 3899. Cluster bootstrap samples whole prompt means without within-prompt resampling.', 'longitudinal_eligible': False, 'runs': results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='verify committed analysis without rewriting it')
    args = parser.parse_args()
    result = analyze()
    if args.check:
        require(json.loads((ROOT / 'analysis.json').read_text()) == result, 'analysis.json differs from recomputation')
        print('Verified archive hashes, 300 paired receipts, 600 generations, judge scores, original summaries, and prompt-cluster analysis.')
    else:
        (ROOT / 'analysis.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
