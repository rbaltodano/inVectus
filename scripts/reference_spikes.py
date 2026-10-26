"""Python reference, operating exclusively on inVectus' copied 2000-row sample."""
import json
import os
from pathlib import Path
import sys
import time
import numpy as np
from sklearn.neighbors import NearestNeighbors
from sklearn.decomposition import PCA
import umap
import hdbscan

out = Path('.spike-data')
x = np.load(out/'vectors.npy')
t = time.perf_counter()
knn = NearestNeighbors(n_neighbors=30, metric='cosine', n_jobs=8).fit(x)
dist, idx = knn.kneighbors(x)
knn_s = time.perf_counter()-t
idx.astype('<u4').tofile(out/'knn.u32')
dist.astype('<f4').tofile(out/'knn.f32')
# Numba warmup is separated from timed projection.
t = time.perf_counter()
umap.UMAP(n_neighbors=10, n_epochs=10, random_state=0).fit_transform(x[:50])
warm_s = time.perf_counter()-t
results = {'items':len(x),'knn_s':knn_s,'numba_warm_s':warm_s,'projections':[],'clusters':[]}
initial = PCA(n_components=2, random_state=0).fit_transform(x).astype(np.float32)
initial = initial / np.max(np.abs(initial)) * 10
initial += np.random.default_rng(42).normal(0, 0.0001, initial.shape).astype(np.float32)
initial.astype('<f4').tofile(out/'umap-init.f32')
for dims in [10,2]:
    t = time.perf_counter()
    y = umap.UMAP(n_neighbors=30,n_components=dims,min_dist=0.0 if dims==10 else 0.1,metric='cosine',precomputed_knn=(idx,dist,None),random_state=0,n_epochs=200,init=initial.copy() if dims==2 else 'spectral').fit_transform(x)
    seconds=time.perf_counter()-t
    np.save(out/f'python-umap-{dims}.npy',y)
    y.astype('<f4').tofile(out/f'python-umap-{dims}.f32')
    results['projections'].append(dict(dimensions=dims,seconds=seconds))
    if dims==10:
        for method in ['leaf','eom']:
            t=time.perf_counter()
            hd=hdbscan.HDBSCAN(min_cluster_size=30,min_samples=10,cluster_selection_method=method,core_dist_n_jobs=8).fit(y)
            seconds=time.perf_counter()-t
            np.save(out/f'python-hdbscan-{method}.npy',hd.labels_)
            results['clusters'].append(dict(selection=method,seconds=seconds,clusters=len(set(hd.labels_)-{-1}),noise=float(np.mean(hd.labels_<0))))
Path('docs/spikes/python-results.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results),flush=True)
