"""Copy a deterministic <=10k-paper sample from the gapmap reference for P1 development tests.

Read-only DuckDB connection; writes only to ignored .spike-data/p1/. Never modifies the reference.
"""
from pathlib import Path
import hashlib
import json
import duckdb

SOURCE = Path.home() / 'Documents/Medical Paper Vector Database/data/gapmap.duckdb'
OUT = Path('.spike-data/p1')
N = 5000
OUT.mkdir(parents=True, exist_ok=True)
con = duckdb.connect(str(SOURCE), read_only=True)
# Hash-ordered selection: deterministic, independent of storage order, no full copy.
q = f"""SELECT pmid, title, abstract, year, pub_date, journal, doi,
            array_to_string(pub_types, '; ') AS pub_types, is_synthesis, is_primary
     FROM papers WHERE abstract IS NOT NULL AND length(abstract) > 0
     ORDER BY hash(pmid) LIMIT {N}"""
csv = OUT / 'papers-5k.csv'
pq = OUT / 'papers-5k.parquet'
con.execute(f"COPY ({q}) TO '{csv}' (HEADER, DELIMITER ',')")
con.execute(f"COPY ({q}) TO '{pq}' (FORMAT PARQUET)")
meta = dict(items=N, query='papers with abstracts ordered by hash(pmid), first 5000',
            csv_sha256=hashlib.sha256(csv.read_bytes()).hexdigest())
(OUT / 'manifest.json').write_text(json.dumps(meta, indent=2) + '\n')
print(meta)
