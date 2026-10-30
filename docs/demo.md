# Urban Futures demo

This demo uses **540 original fictional records** about urban research across
12 topics. It includes deliberately uneven demand/coverage, older coverage and
related topics to exercise the interface. It contains no private collection,
third-party abstracts, real study findings, external links or downloaded dataset.

The demo demonstrates mechanics. Its scores, calibration and apparent gaps are
not evidence of research quality or validated real-world opportunities.

## 1. Build the CLI and install a model

Follow [development setup](development.md). From the repository root:

```sh
npm ci
npm run build
cargo build --workspace --release --locked
```

BGE-small must be installed before embedding. If it is already available in your
model store, no network is needed. Otherwise choose an explicit local installation
or the approximately 134 MB download described in [model setup](development.md#embedding-models).

On Windows the binary path ends in `.exe`; pass it explicitly with
`--cli target/release/invectus.exe` when using the demo builder.

## 2. Generate and build the project

```sh
python3 scripts/create_demo.py --build
```

The builder uses only Python's standard library and the compiled CLI. It forces
offline mode and runs import, embeddings, no-LLM calibration, regions, and a gap scan.
An absent model produces an error; the builder never downloads one automatically.

Outputs, all ignored by Git:

- `.spike-data/public-demo/urban-futures.csv`
- `.spike-data/public-demo/Urban Futures Demo.invectus/`
- `.spike-data/public-demo/demo-report.json`

`--output /another/local/directory` selects another location. Re-running with the
same output path updates the synthetic source, reuses unchanged embeddings and
recomputes calibration/regions/gaps. Close the app before rebuilding with the CLI, since it can hold the database open.
Reimports retain source rows that were removed from a later fixture version; use a
new `--output` directory when you want a completely fresh project.

To generate just the CSV without any model or CLI:

```sh
python3 scripts/create_demo.py
```

## 3. Open it in the desktop app

Run `npm run tauri dev`, choose **Open project…**, and select
`Urban Futures Demo.invectus` from the output folder. If using a custom
`INVECTUS_MODELS_DIR`, launch the app with the same value used by the CLI.

To import the CSV manually instead, create a new project and map:

| Field | CSV column |
| --- | --- |
| ID | `id` |
| Title | `title` |
| Text | `text` |
| Date | `date` |
| Role rules | `demand=demand,coverage=coverage` from `role` |

Leave token chunking selected and index with BGE-small. The text fits a small model;
the demo is not a long-document chunking benchmark.

## 4. Walk through the workspace

1. Open **Gaps**. Filter by coverage, stale coverage or bridges and inspect the map.
2. Open a heat, transit or stormwater coverage gap. Inspect demand/coverage counts,
   the activity timeline, supporting items and score breakdown.
3. Choose **Save gap**. Under **Decision and note**, choose **Selected to work on**,
   write an explicitly illustrative note and choose **Save decision**.
4. Open **Saved** to see the bookmark and your decision. These are independent:
   removing a bookmark does not remove the decision.
5. Open **Search**, enter `Reflective roofs and street trees to reduce neighborhood heat exposure`,
   and press Enter. Read the assessment and supporting matches. Try
   `Step-free bus stops connected by accessible sidewalks` as another query.

Keep the network off. Outside-source checks are neither necessary nor appropriate
to validate invented studies. Optional local descriptions require a separately
installed language model and are not needed for this walkthrough.

## Screenshots

The images in [screenshots/](screenshots/) are direct captures of the native macOS
app using this fictional project, an installed BGE-small model and no network.
The sidebar was filtered to the demo project. Gap bookmarks and the note were
created manually through the app; the generator does not prefill human decisions.

They show actual engine output. Exact labels, positions, scores and selected gap
IDs may vary with engine versions. No scores or result text were composited into
the images. Captures are exported as PNG with EXIF/text metadata removed; compressed
pixel data is preserved during metadata removal. Screenshot capture is separate from the CLI builder.
