"""Run timed experiments serially after compilation; store only metrics in Git."""
import json
import os
from pathlib import Path
import subprocess
import sys

root = Path('docs/spikes')
env = dict(os.environ, NUMBA_NUM_THREADS='8', OPENBLAS_NUM_THREADS='8', RAYON_NUM_THREADS='8')
def run(args, dest):
    print('Running', ' '.join(args), flush=True)
    result = subprocess.run(args, env=env, text=True, capture_output=True, check=True)
    if result.stderr:
        Path('.spike-data', dest+'.stderr').write_text(result.stderr)
    lines = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    assert lines, result.stdout
    data = lines[0] if len(lines)==1 else lines
    (root/(dest+'.json')).write_text(json.dumps(data,indent=2)+'\n')
    print(json.dumps(data),flush=True)
run(['.spike-data/venv/bin/python','-B','scripts/reference_spikes.py'],'python-results')
for model in ['nomic','bge']:
    run(['spikes/target/release/embedding',model],model+'-results')
run(['spikes/target/release/lance'],'lance-results')
run(['spikes/target/release/regions','cluster'],'clustering-results')
run(['spikes/target/release/regions','umap'],'umap-results')
run(['.spike-data/venv/bin/python','-B','scripts/compare_spikes.py'],'comparison-results')
