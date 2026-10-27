"""P1 search check: ANN (default nprobes) vs exhaustive top-10 overlap, and latency, using
held-out paper titles as queries. Usage: python3 scripts/p1_search_eval.py PROJECT COLLECTION QUERIES.json
Runs the release CLI offline; INVECTUS_MODELS_DIR must point at installed models."""
import json, subprocess, sys, statistics as st

project, collection, qpath = sys.argv[1:4]
queries = json.load(open(qpath))
cli = 'target/release/invectus'

def run(q, *extra):
    out = subprocess.run([cli, '--offline', 'search', '-p', project, '-c', collection, q, '-k', '10', '--json', *extra],
                         check=True, capture_output=True, text=True).stdout
    return json.loads(out)

overlap, ann_ms, ex_ms, embed_ms, top1 = [], [], [], [], 0
for q in queries:
    a, x = run(q), run(q, '--exhaustive')
    ia = [h['item']['item_id'] for h in a['hits']]
    ix = [h['item']['item_id'] for h in x['hits']]
    overlap.append(len(set(ia) & set(ix)) / max(len(ix), 1))
    top1 += ia[:1] == ix[:1]
    ann_ms.append(a['retrieve_ms']); ex_ms.append(x['retrieve_ms']); embed_ms.append(a['embed_ms'])
    mode = a['mode']
pct = lambda v, p: sorted(v)[int(p * (len(v) - 1))]
print(json.dumps(dict(queries=len(queries), ann_mode=mode, recall_at_10_vs_exhaustive=st.mean(overlap),
    min_recall_at_10=min(overlap), top1_agreement=top1 / len(queries),
    ann_retrieve_ms_p50=pct(ann_ms, .5), ann_retrieve_ms_p95=pct(ann_ms, .95),
    exhaustive_retrieve_ms_p50=pct(ex_ms, .5), exhaustive_retrieve_ms_p95=pct(ex_ms, .95),
    query_embed_ms_p50=pct(embed_ms, .5)), indent=2))
