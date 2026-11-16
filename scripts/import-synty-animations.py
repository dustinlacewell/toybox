"""Stage Unity-packaged Synty animation FBXs for Toybox's native extraction.

Usage: python scripts/import-synty-animations.py <unitypackage> <pack> <stage-root> <FBX2glTF.exe>
Only Animations/**/*.fbx are imported; demo geometry and scripts are ignored.
The source package's GUIDs, paths and Unity clip settings remain in provenance.
Requires PyYAML. All extraction/conversion scratch belongs on the staging drive.
"""
import concurrent.futures
import hashlib
import bisect
import math
import struct
import json
import pathlib
import re
import subprocess
import sys
import tarfile
import time
import os

import yaml


def pose_clip(doc, binary, setting):
    """FBX2glTF omits constant pose takes; retain their authored node values."""
    binary = bytearray(binary)
    doc.setdefault("bufferViews", [])
    doc.setdefault("accessors", [])
    duration = max(1, float(setting["lastFrame"]) - float(setting["firstFrame"])) / 30

    def add(values, kind):
        while len(binary) % 4:
            binary.append(0)
        payload = b"".join(struct.pack("<" + "f" * len(v), *v) for v in values)
        view = len(doc["bufferViews"])
        doc["bufferViews"].append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(payload)})
        binary.extend(payload)
        accessor = {"bufferView": view, "componentType": 5126, "count": len(values), "type": kind}
        if kind == "SCALAR":
            accessor.update(min=[0], max=[duration])
        index = len(doc["accessors"])
        doc["accessors"].append(accessor)
        return index

    times = add([(0,), (duration,)], "SCALAR")
    clip = {"name": setting["name"], "samplers": [], "channels": []}
    joints = sorted({joint for skin in doc.get("skins", []) for joint in skin["joints"]})
    for joint in joints:
        node = doc["nodes"][joint]
        if "matrix" in node:
            raise ValueError("Pose needs matrix decomposition")
        for prop, default in [("translation", [0, 0, 0]), ("rotation", [0, 0, 0, 1]), ("scale", [1, 1, 1])]:
            values = node.get(prop, default)
            output = add([values, values], "VEC4" if prop == "rotation" else "VEC3")
            sampler = len(clip["samplers"])
            clip["samplers"].append({"input": times, "output": output, "interpolation": "LINEAR"})
            clip["channels"].append({"sampler": sampler, "target": {"node": joint, "path": prop}})
    doc["animations"] = [clip]
    doc["buffers"][0]["byteLength"] = len(binary)
    return doc, bytes(binary)


def split_clips(doc, binary, settings, frames, fallback_name):
    """Honor Unity subclip cuts, resampling cut boundaries without losing keys."""
    source = [clip for clip in doc.get("animations", []) if clip.get("channels")]
    if len(source) != 1 or len(frames) != 1:
        raise ValueError("Expected one FBX take with an explicit frame range")
    original = source[0]
    begin, end = map(float, frames[0])
    duration = max(doc["accessors"][sampler["input"]]["max"][0] for sampler in original["samplers"])
    fps = (end - begin) / duration
    if fps <= 0 or not math.isfinite(fps):
        raise ValueError("Invalid FBX timing")
    binary = bytearray(binary)

    def read(index):
        accessor = doc["accessors"][index]
        view = doc["bufferViews"][accessor["bufferView"]]
        if accessor["componentType"] != 5126 or "sparse" in accessor:
            raise ValueError("Expected float animation samples")
        width = {"SCALAR": 1, "VEC3": 3, "VEC4": 4}[accessor["type"]]
        offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        stride = view.get("byteStride", width * 4)
        return [struct.unpack_from("<" + "f" * width, binary, offset + i * stride) for i in range(accessor["count"])]

    def append(values, kind, times=False):
        while len(binary) % 4:
            binary.append(0)
        payload = b"".join(struct.pack("<" + "f" * len(value), *value) for value in values)
        view = len(doc["bufferViews"])
        doc["bufferViews"].append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(payload)})
        binary.extend(payload)
        accessor = {"bufferView": view, "componentType": 5126, "count": len(values), "type": kind}
        if times:
            accessor.update(min=[values[0][0]], max=[values[-1][0]])
        index = len(doc["accessors"])
        doc["accessors"].append(accessor)
        return index

    clips = []
    for setting in settings or [{"name": fallback_name, "firstFrame": begin, "lastFrame": end}]:
        first = max(begin, float(setting["firstFrame"]))
        last = min(end, float(setting["lastFrame"]))
        if last <= first:
            raise ValueError("Unity clip is outside the FBX take")
        start, stop = (first - begin) / fps, (last - begin) / fps
        clip = json.loads(json.dumps(original))
        clip["name"] = setting.get("name", fallback_name)
        clip["extras"] = {"unityClip": setting, "fbxFrameRange": [begin, end], "importedFrameRange": [first, last], "framesPerSecond": fps}
        paths = {channel["sampler"]: channel["target"]["path"] for channel in clip["channels"]}
        for index, sampler in enumerate(clip["samplers"]):
            interpolation = sampler.get("interpolation", "LINEAR")
            if interpolation not in ("LINEAR", "STEP"):
                raise ValueError("Unsupported FBX interpolation")
            times = [value[0] for value in read(sampler["input"])]
            values = read(sampler["output"])
            sample_times = [start] + [time for time in times if start + 1e-6 < time < stop - 1e-6] + [stop]

            def sample(time):
                i = max(0, min(len(times) - 2, bisect.bisect_right(times, time) - 1))
                if time <= times[0]:
                    return values[0]
                if time >= times[-1]:
                    return values[-1]
                a, b = values[i], values[i + 1]
                alpha = (time - times[i]) / (times[i + 1] - times[i])
                if interpolation == "STEP":
                    return a
                if paths[index] == "rotation":
                    dot = sum(x * y for x, y in zip(a, b))
                    if dot < 0:
                        b = tuple(-v for v in b)
                        dot = -dot
                    if dot < 0.9995:
                        angle = math.acos(max(-1, min(1, dot)))
                        return tuple((math.sin((1 - alpha) * angle) * x + math.sin(alpha * angle) * y) / math.sin(angle) for x, y in zip(a, b))
                    value = [x * (1 - alpha) + y * alpha for x, y in zip(a, b)]
                    length = math.sqrt(sum(x * x for x in value))
                    return tuple(x / length for x in value)
                return tuple(x * (1 - alpha) + y * alpha for x, y in zip(a, b))

            output_type = doc["accessors"][sampler["output"]]["type"]
            samples = [sample(time) for time in sample_times]
            sampler["input"] = append([(max(0, time - start),) for time in sample_times], "SCALAR", True)
            sampler["output"] = append(samples, output_type)
        clips.append(clip)
    doc["animations"] = clips
    doc["buffers"][0]["byteLength"] = len(binary)
    return doc, bytes(binary)


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def stage(package, pack, root, converter):
    unpacked = root / "unity" / pack
    # Stream once. Random access through a gzip tar repeatedly decompresses it.
    with tarfile.open(package, "r|gz") as archive:
        for member in archive:
            if not member.isfile():
                continue
            parts = member.name.split("/")
            if len(parts) != 2 or not re.fullmatch("[a-fA-F0-9]{32}", parts[0]) or parts[1] not in ("asset", "asset.meta", "pathname"):
                continue
            target = unpacked / parts[0] / parts[1]
            target.parent.mkdir(parents=True, exist_ok=True)
            if not target.exists():
                target.write_bytes(archive.extractfile(member).read())
    jobs = []
    for pathname in sorted(unpacked.glob("*/pathname")):
        source = pathname.read_text(encoding="utf-8-sig")
        if "/Animations/" not in source or not source.lower().endswith(".fbx"):
            continue
        meta = yaml.load(pathname.with_name("asset.meta").read_bytes(), Loader=yaml.CSafeLoader)["ModelImporter"]
        clips = meta.get("animations", {}).get("clipAnimations", [])
        jobs.append({"guid": pathname.parent.name, "path": source, "meta": clips, "fbx": pathname.with_name("asset")})
    print(json.dumps({"pack": pack, "animationSources": len(jobs), "declaredClips": sum(len(job["meta"]) for job in jobs),
                      "mirroredClips": sum(bool(clip.get("mirror")) for job in jobs for clip in job["meta"])}), flush=True)
    folder = root / "library" / pack / "sources"
    folder.mkdir(parents=True, exist_ok=True)

    def convert(job):
        guid = job["guid"]
        stem = pathlib.PurePosixPath(job["path"]).stem
        name = re.sub(r"[^A-Za-z0-9_-]", "_", stem) + "_" + guid[:8]
        if job["fbx"].stat().st_size < 200 and job["fbx"].read_bytes().startswith(b"version https://git-lfs"):
            return {"skipped": job["path"], "reason": "Archive contains a Git LFS pointer instead of the FBX data", "clips": len(job["meta"])}
        work = root / "conversion" / pack / guid
        work.mkdir(parents=True, exist_ok=True)
        fbx = work / (name + ".fbx")
        if not fbx.exists():
            fbx.write_bytes(job["fbx"].read_bytes())
        destination = folder / (name + ".gltf")
        log_file = work / "conversion.log"
        existing = list(work.rglob("*.gltf"))
        if existing and log_file.exists():
            log = log_file.read_text(encoding="utf-8")
            completed = None
        else:
            completed = subprocess.run([str(converter), "-i", str(fbx), "-o", str(work / name), "--pbr-metallic-roughness", "--separate-textures"],
                                       capture_output=True, text=True, errors="replace", creationflags=subprocess.CREATE_NO_WINDOW)
            log = completed.stdout + "\n" + completed.stderr
            log_file.write_text(log, encoding="utf-8")
        if completed and completed.returncode:
            raise RuntimeError(f"{stem}: FBX conversion failed: {log[-1000:]}")
        output = next(work.rglob("*.gltf"), None)
        if output is None:
            raise RuntimeError(f"{stem}: converter produced no glTF")
        doc = json.loads(output.read_text(encoding="utf-8"))
        clips = [clip for clip in doc.get("animations", []) if clip.get("channels")]
        if doc.get("meshes"):
            raise RuntimeError(f"{stem}: unexpected geometry in animation source")
        # One FBX can define several Unity subclips (wind-up, hit, recovery).
        # Preserve their names and frame cuts rather than only the whole take.
        frames = re.findall(r"Animation .*: \[(-?[\d.]+) - (-?[\d.]+)\]", log)
        for index, buffer in enumerate(doc.get("buffers", [])):
            binary = (output.parent / buffer["uri"]).read_bytes()
            if len(doc["buffers"]) != 1:
                raise RuntimeError(f"{stem}: expected one FBX buffer")
            if not clips:
                if len(job["meta"]) != 1 or not any(term in stem.lower() for term in ("pose", "tpose")):
                    raise RuntimeError(f"{stem}: no channels and no explicit static pose")
                doc, binary = pose_clip(doc, binary, job["meta"][0])
                frames = [(job["meta"][0]["firstFrame"], job["meta"][0]["lastFrame"])]
            doc, binary = split_clips(doc, binary, job["meta"], frames, stem)
            buffer["uri"] = name + ".bin"
            (folder / buffer["uri"]).write_bytes(binary)
        write_json(destination, doc)
        return {"entry": {"kind": "animation", "id": "uid://synty_source_" + guid, "pack": pack, "category": "sources", "file": name + ".gltf"},
                "provenance": {"guid": guid, "sourceFile": job["path"], "sourceName": stem, "unityPackage": package.name,
                               "fbxSha256": hashlib.sha256(job["fbx"].read_bytes()).hexdigest(), "unityClipSettings": job["meta"]}}

    results = []
    errors = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        tasks = {pool.submit(convert, job): job for job in jobs}
        for task in concurrent.futures.as_completed(tasks):
            try:
                results.append(task.result())
            except Exception as error:
                errors.append({"file": tasks[task]["path"], "error": str(error)})
            if (len(results) + len(errors)) % 100 == 0:
                print(json.dumps({"pack": pack, "processed": len(results) + len(errors), "errors": len(errors)}), flush=True)
    results.sort(key=lambda result: result.get("entry", {}).get("id", result.get("skipped", "")))
    write_json(root / (pack + "-staged.json"), {"results": results, "errors": errors})
    if errors:
        raise RuntimeError(f"{len(errors)} sources need attention; see {pack}-staged.json")
    write_json(root / (pack + "-entries.json"), [result["entry"] for result in results if "entry" in result])
    print(json.dumps({"pack": pack, "staged": sum("entry" in result for result in results), "skipped": sum("skipped" in result for result in results)}), flush=True)


def publish(stage_root, library_root):
    stage_root = pathlib.Path(stage_root)
    root = pathlib.Path(library_root).resolve()
    manifest_path = root / "_library_config/catalog.json"
    previous = manifest_path.read_bytes()
    manifest = json.loads(previous)
    staged = json.loads((stage_root / "_library_config/catalog.json").read_text(encoding="utf-8"))
    provenance = {}
    skipped = []
    for pack in ("synty_base_locomotion", "synty_sword_combat"):
        report = json.loads((stage_root / (pack + "-staged.json")).read_text(encoding="utf-8"))
        if report["errors"]:
            raise ValueError("Conversion errors remain")
        for result in report["results"]:
            if "entry" in result:
                provenance[result["entry"]["id"]] = result["provenance"]
            else:
                skipped.append(result)
    new_entries = []
    counts = {}
    ranges_clamped = 0
    for entry in staged["assets"]:
        if entry.get("kind") != "animation" or not entry.get("standalone"):
            raise ValueError("Run the native reindex extractor on staging before publishing")
        source = stage_root / "library" / entry["pack"] / entry["category"] / entry["file"]
        doc = json.loads(source.read_text(encoding="utf-8"))
        if any(field in doc for field in ("meshes", "images", "textures", "materials")):
            raise ValueError("Expected a pure animation")
        metadata = doc["extras"]["toyboxAnimation"]
        origin = provenance[metadata["clip"]["sourceId"]]
        metadata["clip"]["sourceName"] = origin["sourceName"]
        metadata["provenance"] = {key: value for key, value in origin.items() if key != "unityClipSettings"}
        setting = doc["animations"][0].get("extras", {}).get("unityClip", {})
        metadata["provenance"]["unityClipName"] = setting.get("name", doc["animations"][0]["name"])
        frame_range = doc["animations"][0].get("extras", {}).get("importedFrameRange")
        if frame_range and frame_range != [setting.get("firstFrame"), setting.get("lastFrame")]:
            ranges_clamped += 1
        folders = origin["sourceFile"].split("/Animations/", 1)[1].split("/")[:-1]
        folders = [part for part in folders if part not in ("Polygon", "Sidekick", "Masculine", "Feminine", "Neutral")]
        category = re.sub(r"[^a-z0-9]+", "_", (folders[0] if folders else "idle").lower()).strip("_")
        if "pose" in origin["sourceName"].lower():
            category = "poses"
        entry = {**entry, "category": category}
        target = (root / "library" / entry["pack"] / category / entry["file"]).resolve()
        if not target.is_relative_to(root / "library" / entry["pack"]):
            raise ValueError("Animation target escapes its pack")
        binary = (source.parent / doc["buffers"][0]["uri"]).read_bytes()
        if len(binary) != doc["buffers"][0]["byteLength"]:
            raise ValueError("Animation buffer length mismatch")
        for accessor in doc["accessors"]:
            view = doc["bufferViews"][accessor["bufferView"]]
            width = {"SCALAR": 1, "VEC3": 3, "VEC4": 4}[accessor["type"]]
            offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
            stride = view.get("byteStride", width * 4)
            if offset + (accessor["count"] - 1) * stride + width * 4 > len(binary):
                raise ValueError("Truncated animation samples")
        for destination, payload in [(target.parent / doc["buffers"][0]["uri"], binary), (target, (json.dumps(doc, indent=2) + "\n").encode("utf-8"))]:
            destination.parent.mkdir(parents=True, exist_ok=True)
            if destination.exists():
                if destination.read_bytes() != payload:
                    raise ValueError(f"Refusing to replace a differing existing file: {destination}")
                continue
            temporary = destination.with_suffix(destination.suffix + ".import.tmp")
            temporary.write_bytes(payload)
            os.replace(temporary, destination)
        new_entries.append(entry)
        counts[entry["pack"]] = counts.get(entry["pack"], 0) + 1
    by_id = {entry["id"]: entry for entry in manifest["assets"]}
    for entry in new_entries:
        if entry["id"] in by_id and by_id[entry["id"]] != entry:
            raise ValueError("An animation ID is already registered differently")
        if entry["id"] not in by_id:
            manifest["assets"].append(entry)
    if manifest_path.read_bytes() != previous:
        raise ValueError("Live manifest changed during publication; rerun")
    backup = manifest_path.with_name(f"catalog-before-synty-animation-import-{time.time_ns()}.json")
    backup.write_bytes(previous)
    temporary = manifest_path.with_suffix(".animation-import.tmp")
    write_json(temporary, manifest)
    os.replace(temporary, manifest_path)
    for pack, name, color in [("synty_base_locomotion", "Synty Base Locomotion", "708cafff"), ("synty_sword_combat", "Synty Sword Combat", "a97c64ff")]:
        path = root / "library" / pack / "pack.json"
        if not path.exists():
            write_json(path, {"slug": pack, "name": name, "color": color})
    report = {"imported": counts, "skipped": skipped, "clipRangesClampedToAvailableFrames": ranges_clamped, "manifestBackup": str(backup)}
    write_json(root / "_library_config/synty-animation-import-report.json", report)
    print(json.dumps(report, indent=2))


if __name__ == "__main__" and len(sys.argv) == 4 and sys.argv[1] == "--publish":
    publish(sys.argv[2], sys.argv[3])
elif __name__ == "__main__":
    if len(sys.argv) != 5:
        raise SystemExit(__doc__)
    package, pack, root, converter = sys.argv[1:]
    if not re.fullmatch(r"[a-z0-9_]+", pack):
        raise ValueError("Pack must be a directory slug")
    stage(pathlib.Path(package), pack, pathlib.Path(root), pathlib.Path(converter))
