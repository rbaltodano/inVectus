"""Copy <=10k rows from the reference; never write to its DB/vector store."""
from pathlib import Path
import json
import time
import hashlib
import duckdb
import lancedb
import numpy as np

SOURCE = Path.home() / 'Documents/Medical Paper Vector Database/data'
OUT = Path('.spike-data')
t = lancedb.connect(str(SOURCE / 'vectors')).open_table('embeddings_nomic')
# Lance scan limit applies before materialization: no full-corpus copy.
a = t.search().limit(2000).to_arrow()
ids = a['pmid'].to_numpy()
x = np.asarray(a['vector'].to_pylist(), dtype=np.float32)
x /= np.linalg.norm(x, axis=1, keepdims=True)
x.astype('<f4').tofile(OUT / 'vectors.f32')
np.save(OUT / 'vectors.npy', x)
con = duckdb.connect(str(SOURCE / 'gapmap.duckdb'), read_only=True)
con.register('sample_ids', a.select(['pmid']))
rows = con.execute("SELECT p.pmid, p.title, coalesce(p.abstract,'') FROM papers p JOIN sample_ids s USING(pmid)").fetchall()
byid = {i: f'{title}. {abstract}' for i, title, abstract in rows}
(OUT / 'texts.json').write_text(json.dumps([byid[int(i)] for i in ids]))
(OUT / 'ids.json').write_text(json.dumps(ids.tolist()))
meta = dict(items=len(ids), dimensions=x.shape[1], sample='first 2000 rows in Lance scan order; not random or representative', vector_sha256=hashlib.sha256((OUT/'vectors.f32').read_bytes()).hexdigest(), ids_sha256=hashlib.sha256((OUT/'ids.json').read_bytes()).hexdigest())
Path('docs/spikes/sample-manifest.json').write_text(json.dumps(meta, indent=2)+'\n')
print(meta, flush=True)
