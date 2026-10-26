"""Compare outputs, never infer correctness from speed alone."""
import json
from pathlib import Path
import numpy as np
from sklearn.manifold import trustworthiness
from sklearn.metrics import adjusted_rand_score
from sklearn.neighbors import NearestNeighbors

out = Path('.spike-data')
x = np.load(out/'vectors.npy')
labels = np.array(json.loads((out/'rust-labels.json').read_text()))
comparison = {}
for method in ['leaf','eom']:
    ref = np.load(out/f'python-hdbscan-{method}.npy')
    best=[]
    for c in sorted(set(ref)-{-1}):
        a=ref==c
        best.append(max((np.sum(a & (labels==d))/np.sum(a | (labels==d)) for d in set(labels)-{-1}),default=0))
    comparison[method] = dict(ari=float(adjusted_rand_score(ref,labels)),mean_best_jaccard=float(np.mean(best)),noise_agreement=float(np.mean((ref<0)==(labels<0))))

maps={}
for name in ['python','rust-0','rust-1']:
    y = np.load(out/'python-umap-2.npy') if name=='python' else np.fromfile(out/f'rust-umap-{name[-1]}.f32',dtype='<f4').reshape(-1,2)
    maps[name]=dict(trustworthiness_15=float(trustworthiness(x,y,n_neighbors=15,metric='cosine')),finite=bool(np.isfinite(y).all()),axis_std=np.std(y,axis=0).tolist())
Path('docs/spikes/comparison-results.json').write_text(json.dumps(dict(clustering=comparison,maps=maps),indent=2)+'\n')
print(json.dumps(dict(clustering=comparison,maps=maps)),flush=True)
