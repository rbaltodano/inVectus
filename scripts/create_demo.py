#!/usr/bin/env python3
"""Generate fictional urban-research records; optionally build an offline inVectus project.

Python standard library only. No downloads. Run from the repository root:
    python3 scripts/create_demo.py
    python3 scripts/create_demo.py --build --cli target/release/invectus
Generated CSV, project and reports stay under .spike-data/public-demo/ by default.
"""
import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import subprocess

# Original fictional content, authored for this demo. No real studies or people are represented.
TOPICS = [
    ("Urban heat", "street trees and reflective roofs", "reduce summer heat exposure", "surface temperatures and pedestrian comfort", "demand"),
    ("Accessible transit", "step-free bus stops and connected sidewalks", "improve access to public transport", "walking distances and wheelchair access", "demand"),
    ("Stormwater", "rain gardens and permeable streets", "manage intense rainfall", "runoff volumes and flood resilience", "demand"),
    ("Air quality", "low-emission streets and roadside sensors", "reduce traffic-related pollution", "particulate concentrations and exposure", "stale"),
    ("Urban biodiversity", "native planting and habitat corridors", "support pollinators in dense neighborhoods", "species diversity and habitat connectivity", "stale"),
    ("Building energy", "heat pumps and apartment insulation", "lower building energy demand", "heating loads and household electricity use", "stale"),
    ("Cycling networks", "protected cycle lanes and safe intersections", "make everyday cycling safer", "route connectivity and traffic conflicts", "mixed"),
    ("Community gardens", "shared gardens and local food networks", "expand access to fresh food", "growing space and seasonal food availability", "mixed"),
    ("Circular materials", "reclaimed timber and reusable building components", "reduce construction waste", "material recovery and embodied emissions", "mixed"),
    ("Water conservation", "greywater reuse and smart irrigation", "conserve water during dry seasons", "water consumption and landscape health", "mixed"),
    ("Housing comfort", "passive ventilation and exterior shading", "improve indoor conditions in apartments", "indoor temperatures and occupant comfort", "mixed"),
    ("Public spaces", "welcoming plazas and shared recreation areas", "improve access to neighborhood public space", "use patterns and social connection", "mixed"),
]
TOPIC_COUNTS = [72, 48, 60, 36, 40, 44, 48, 40, 36, 44, 40, 32]
FOCI = ["long-term performance", "maintenance costs", "equitable access", "seasonal change", "implementation barriers", "neighborhood outcomes", "monitoring design", "resident priorities", "policy tradeoffs"]
CONTEXTS = ["riverside districts", "compact neighborhoods", "outer suburbs", "historic centers", "mixed-use blocks", "school surroundings", "station precincts", "hillside communities"]
METHODS = ["a comparative field study", "a seasonal monitoring program", "a neighborhood survey", "a small pilot intervention", "a longitudinal evaluation", "a spatial planning analysis"]


def generate(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=["id", "title", "text", "date", "role", "topic"])
        writer.writeheader()
        for topic_index, (topic, approach, goal, measures, pattern) in enumerate(TOPICS):
            for i in range(TOPIC_COUNTS[topic_index]):
                coverage = pattern != "demand" and i % 3 == 0
                context = CONTEXTS[i % len(CONTEXTS)]
                focus = FOCI[i // len(CONTEXTS)]
                method = METHODS[(i // len(CONTEXTS) + topic_index) % len(METHODS)]
                year = 2015 + i % 4 if coverage and pattern == "stale" else 2021 + i % 6
                if coverage:
                    title = f"{topic}: evaluating {focus} in {context}"
                    body = f"This fictional evaluation considers how {approach} can {goal} in {context}. Using {method}, the proposed analysis compares {measures} across intervention and comparison sites. It outlines baseline measurements, maintenance requirements and possible implementation tradeoffs. Scenario {i + 1} focuses on a different block size and monitoring schedule. This is synthetic demonstration content, not a published study or a finding."
                else:
                    title = f"{topic}: {focus} in {context}"
                    body = f"This fictional research proposal asks whether {approach} can {goal} in {context}. Evidence is needed about {measures}, including long-term performance, equitable access and maintenance costs. The proposed design uses {method} and compares alternative neighborhood interventions. Scenario {i + 1} varies the study scale and follow-up period. This is synthetic demonstration content, not a published study or a finding."
                writer.writerow(dict(id=f"demo-{topic_index + 1:02}-{i + 1:02}", title=title, text=body, date=f"{year}-06-15", role="coverage" if coverage else "demand", topic=topic))
    return sum(TOPIC_COUNTS)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path(".spike-data/public-demo"))
    parser.add_argument("--build", action="store_true", help="Build a project using an already installed BGE-small model; never downloads")
    parser.add_argument("--cli", type=Path, default=Path("target/release/invectus"))
    args = parser.parse_args()
    root = args.output.resolve()
    csv_path = root / "urban-futures.csv"
    count = generate(csv_path)
    report = {"synthetic": True, "items": count, "csv_sha256": hashlib.sha256(csv_path.read_bytes()).hexdigest(), "source": "Original fictional records generated by scripts/create_demo.py"}
    print(f"Generated {count} fictional records: {csv_path}", flush=True)
    if args.build:
        cli = args.cli.resolve()
        if not cli.is_file():
            parser.error(f"CLI not found at {cli}; build the workspace first")
        env = dict(os.environ, INVECTUS_OFFLINE="1")
        def run(*command):
            print("Running: " + " ".join(map(str, command[:2])), flush=True)
            result = subprocess.run([str(cli), "--offline", *map(str, command)], env=env, check=False, text=True, capture_output=True)
            if result.returncode:
                raise SystemExit("Demo build failed: " + result.stderr.strip())
            return result.stdout
        project = root / "Urban Futures Demo.invectus"
        if not (project / "project.json").exists():
            run("init", project)
        shared = ["-p", project, "-c", "urban-futures"]
        commands = [
            ("import", ["import", "-p", project, csv_path, "-c", "urban-futures", "--id", "id", "--title", "title", "--text", "text", "--date", "date", "--role-column", "role", "--role-rules", "demand=demand,coverage=coverage", "--json"]),
            ("embed", ["embed", *shared, "--threads", "4", "--json"]),
            ("calibration", ["calibrate", *shared, "--sample", "100", "--threads", "4", "--json"]),
            ("regions", ["regions", "run", *shared, "--json"]),
            ("gaps", ["gaps", "scan", *shared, "--json"]),
        ]
        for key, command in commands:
            report[key] = json.loads(run(*command))
        report["project"] = str(project)
        print(f"Open this project in the desktop app: {project}", flush=True)
    (root / "demo-report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
