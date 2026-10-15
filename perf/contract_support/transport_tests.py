"""Synthetic inputs exercise the checker only, never establish GPU evidence."""
from __future__ import annotations

import copy
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from .output_extractors import OUTPUT_COUNTERS, PAINTER_COUNTERS, HEAP_COUNTERS, MEASURED_POLICY_COUNTERS
from .output_tests import _set_path, _delete_path
from .transport_extractors import PORTABLE_TRANSPORTS, extract_transport
from .invariants import judge_runner_invariants
from .schema import load_catalog, load_scenario


def _self_test_transports(root: Path) -> list[str]:
    errors = []
    for scenario_id in load_catalog(root).get("nana_transport_ids", []):
        scenario = load_scenario(scenario_id, root)
        workload = scenario["params"]["workload"]
        portable = workload in PORTABLE_TRANSPORTS
        case = {"status":"ok", "adapter":"synthetic checker input; never actual platform evidence"}
        timing = {"samples":240,"p50":1.0,"p95":2.0,"p99":3.0}
        if portable:
            dynamic = workload in {"overlay-changing","overlay-multi","web-changing"}
            consumers = 2 if workload == "overlay-multi" else 1
            producer_budget = {"ui_renders":240,"submissions":240,"content_revisions":240} if dynamic else {"idle_reuse_frames":240}
            consumer_budget = {"extra_passes":240*consumers,"gpu_convert_passes":240*consumers,"submissions":240*consumers} if dynamic else {"deferred":240} if workload=="overlay-slow" else {"idle_reuse_frames":240}
            for owner,budget in (("producer_work",producer_budget),("consumer_work",consumer_budget)):
                for key in OUTPUT_COUNTERS:
                    matching=[row for row in scenario["invariants"] if row["path"]==owner+"."+key]
                    if len(matching)!=1 or matching[0].get("op")!="eq" or matching[0].get("value")!=budget.get(key,0) or matching[0].get("status")!="required":
                        errors.append(f"{scenario_id}: missing/weakened independent {owner}.{key} budget")
            case.update(evidence="actual-gpu-transfer-mock-foreign-retirement",params=scenario["params"].copy(),warmup_frames=30,
                producer_work={key:0 for key in OUTPUT_COUNTERS},consumer_work={key:0 for key in OUTPUT_COUNTERS},
                measurement_gpu_policy={key:0 for key in MEASURED_POLICY_COUNTERS},timing_samples=240,
                producer_call_ms=timing.copy(),consumer_call_ms=timing.copy(), foreign_gpu_waits=None,
                producer_painter={"status":"measured" if dynamic else "skipped", "encoded_frames":240 if dynamic else 0,"skipped_frames":0 if dynamic else 240,"work":{key:0 for key in PAINTER_COUNTERS} if dynamic else None},
                proof={"foreign_retirement":"mock","actual_platform_import":False})
            for key in ("producer_rust_heap","consumer_rust_heap"):
                case[key]={"status":"measured","samples":240,"scope":"synthetic checker input","counts":{name:0 for name in HEAP_COUNTERS}}
        elif workload == "native-host":
            case.update(evidence="actual-native-host-gpu",fixture="native-host-output",native_api="DX12-NT-D3D11",source_calls=240,frames=240,
                producer_work={"ui_renders":240,"draw_calls":240,"submissions":240,"cpu_readbacks":0,"target_recreates":0,"gpu_copies":240,"gpu_copy_bytes":240*1280*720*4},
                consumer_work={"gpu_copies":240,"gpu_copy_bytes":240*1280*720*4,"gpu_ready_waits":240,"gpu_release_signals":240,"target_recreates":0,"validation_readbacks":0},
                proof={"consumer_completed":True,"native_handle_imports":3,"explicit_cpu_fence_waits":0,"production_cpu_readbacks":0},
                producer_window_cpu_ms=timing.copy(),consumer_record_cpu_ms=timing.copy())
        elif workload == "openvr-native":
            case.update(status="active",evidence="actual-openvr-gpu",fixture="openvr-native-output",alpha_encoding="gamma-premultiplied",overlay_premultiplied=True,workload_alpha="opaque",interface="IVROverlay_028",compositor="steamvr",backend="dx12-d3d11",mode="shared_atomic",continuous_reuse_supported=True,compositor_retirement_proven=False,deferred=0,
                warmup_attempts=30,total_attempts=270,total_consumer_updates=270,
                setup_scope={"submitted_to_compositor":1,"consumer_target_allocations":1,"native_call_cpu_ms":{**timing,"samples":1,"status":"measured"}},
                native_call_cpu_ms={"status":"skipped","samples":0,"p50":None,"p95":None,"p99":None})
            for key in ("producer_record_cpu_ms","producer_native_record_cpu_ms","consumer_record_cpu_ms"):
                case[key]={**timing,"status":"measured"}
        else:
            work={key:0 for key in ("ui_renders","gpu_copies","gpu_copy_bytes","extra_passes","submissions","cpu_readbacks","target_recreates","deferred")}
            receipt=lambda count: {"native_handle_imports":count,"ready_observations":count,"foreign_releases":count,"displayed_frames":count,"has_pending_gpu_reads":False}
            consumer=lambda count: {"displayed_frames":count,"submissions":count,"draw_calls":count}
            case.update(evidence="actual-electron-webgpu",fixture="electron-webgpu-output",electron_version="44.7.2",frames=240,total_frames=270,timing_samples=240,timing_ms=timing.copy(),consumer_record_cpu_ms=timing.copy(),
                output_work=work.copy(),setup_work={**work,"target_recreates":6},
                warmup_output_work={**work,"gpu_copies":30,"gpu_copy_bytes":30*640*360*4},
                consumer_work=consumer(240),warmup_consumer_work=consumer(30),total_consumer_work=consumer(270),
                proof=receipt(240),warmup_proof=receipt(30),total_proof=receipt(270))
        if not portable:
            case.update(platform="windows",fixture_version=1,warmup_frames=30,device_generation=1,
                extent={"native-host":[1280,720],"openvr-native":[128,96],"web-native":[640,360]}[workload])
        independent = {"native-host":{"producer_work.draw_calls":("gte",240),"source_calls":("gte",240)},
            "web-native":{"output_work.extra_passes":("eq",0),"output_work.target_recreates":("eq",0),"setup_work.target_recreates":("eq",6),"total_frames":("eq",270)}}.get(workload,{})
        for path,(op,value) in independent.items():
            matching=[row for row in scenario["invariants"] if row["path"]==path]
            if len(matching)!=1 or matching[0].get("op")!=op or matching[0].get("value")!=value or matching[0].get("status")!="required":
                errors.append(f"{scenario_id}: missing/weakened independent actual workload {path}")
        for row in scenario["invariants"]:
            if row["path"] != "platform_conformance":
                _set_path(case,row["path"],row["value"])
        if workload == "web-native":
            # One source submission and one native-copy submission per frame.
            case["output_work"]["submissions"] = case["output_work"]["ui_renders"]+case["output_work"]["gpu_copies"]
        def extract(value):
            report={"transport_cases":{workload:value}} if portable else value
            return extract_transport(scenario,{"output_transport":report},{"output_transport":Path("synthetic checker input")})
        try:
            if judge_runner_invariants(extract(case),root=root).get("decision") != "ok":
                errors.append(f"{scenario_id}: valid synthetic checker input rejected")
        except KeyError as exc:
            errors.append(f"{scenario_id}: valid synthetic checker input malformed: {exc}")
            continue
        for row in scenario["invariants"]:
            bad=copy.deepcopy(case)
            if row["path"] == "platform_conformance":
                continue  # Derived from exact evidence, never copied from input.
            value=row["value"]
            _set_path(bad,row["path"],not value if isinstance(value,bool) else value+(1 if row["op"]=="eq" else -1))
            try:
                if judge_runner_invariants(extract(bad),root=root).get("decision") != "failed":
                    errors.append(f"{scenario_id}: mutation {row['path']} passed")
            except KeyError:
                pass
            bad=copy.deepcopy(case); _delete_path(bad,row["path"])
            try:
                if judge_runner_invariants(extract(bad),root=root).get("decision") != "failed":
                    errors.append(f"{scenario_id}: missing {row['path']} passed")
            except KeyError:
                pass
        for evidence in (None,"mock-contract","actual-gpu","unavailable","actual-window-gpu"):
            bad=copy.deepcopy(case);bad["evidence"]=evidence
            try:
                if extract(bad).get("status")=="ok":errors.append(f"{scenario_id}: false evidence accepted")
            except KeyError:
                pass
        timing_keys=("producer_call_ms","consumer_call_ms") if portable else ("producer_window_cpu_ms","consumer_record_cpu_ms") if workload=="native-host" else ("producer_record_cpu_ms","producer_native_record_cpu_ms","consumer_record_cpu_ms") if workload=="openvr-native" else ("timing_ms","consumer_record_cpu_ms")
        for key in timing_keys:
            for value in (None,True,-1,float("nan"),float("inf"),"1"):
                bad=copy.deepcopy(case);bad[key]["p50"]=value
                try:
                    extract(bad);errors.append(f"{scenario_id}: invalid {key} timing accepted")
                except KeyError:
                    pass
        if not portable:
            negative = {"fixture_version":(None,True,0,2),"warmup_frames":(None,0,29,31),
                "device_generation":(None,True,0),"extent":(None,[1,1],[True,96]),
                "fixture":(None,"generic-trait"),"platform":(None,"macos","linux")}
            if workload == "native-host":
                negative.update({"native_api":(None,"generic-native"),"source_calls":(None,1,239),
                    "producer_window_cpu_ms.samples":(None,1,239),"consumer_record_cpu_ms.samples":(None,0,1,239,241),
                    "producer_work.submissions":(241,480),"producer_work.ui_renders":(239,481)})
            elif workload == "openvr-native":
                negative.update({"alpha_encoding":(None,"straight","linear-premultiplied"),"overlay_premultiplied":(None,False,1),"workload_alpha":(None,"translucent"),"interface":(None,"IVROverlay_027"),"compositor":(None,"mock"),"backend":(None,"generic"),
                    "warmup_attempts":(None,29),"total_attempts":(None,269,271),
                    "native_call_cpu_ms.status":(None,"measured"),"native_call_cpu_ms.samples":(None,1),"native_call_cpu_ms.p50":(0,),
                    "setup_scope.native_call_cpu_ms.samples":(None,0,2),"setup_scope.native_call_cpu_ms.p50":(None,float("nan")),
                    "producer_record_cpu_ms.samples":(None,1,239,241),"producer_native_record_cpu_ms.samples":(None,1,239,241),"consumer_record_cpu_ms.samples":(None,1,239,241),
                    "producer_ui_submissions":(241,480)})
            else:
                negative.update({"electron_version":(None,"44.7.1"),"consumer_record_cpu_ms.samples":(None,0,1,239,241),
                    "timing_samples":(None,0,1,239),"output_work.ui_renders":(241,480),"output_work.submissions":(240,479,481),"total_frames":(None,240,269,271),
                    "setup_work.target_recreates":(None,0,5,7),"warmup_output_work.gpu_copies":(None,0,29,31),
                    "warmup_output_work.gpu_copy_bytes":(None,0,30*640*360*4+1)})
                for name,count in (("warmup_proof",30),("total_proof",270)):
                    for field in ("native_handle_imports","ready_observations","foreign_releases","displayed_frames"):
                        negative[name+"."+field]=(None,count-1,count+1)
                    negative[name+".has_pending_gpu_reads"]=(None,True)
                for name,count in (("warmup_consumer_work",30),("total_consumer_work",270)):
                    for field in ("displayed_frames","submissions","draw_calls"):
                        negative[name+"."+field]=(None,count-1,count+1)
            for path,values in negative.items():
                for value in values:
                    bad=copy.deepcopy(case)
                    if value is None: _delete_path(bad,path)
                    else: _set_path(bad,path,value)
                    try:
                        if judge_runner_invariants(extract(bad),root=root).get("decision") != "failed":
                            errors.append(f"{scenario_id}: bad/missing {path}={value!r} accepted")
                    except KeyError:
                        pass
            # Preserve every other count relationship so each new upper/lower
            # bound is independently exercised rather than masked by a sum error.
            compound = ({"source_calls":241,"producer_window_cpu_ms.samples":241,"producer_work.submissions":241,"producer_work.ui_renders":240}
                if workload == "native-host" else
                {"output_work.ui_renders":241,"output_work.submissions":481}
                if workload == "web-native" else None)
            if compound:
                bad=copy.deepcopy(case)
                for path,value in compound.items(): _set_path(bad,path,value)
                try:
                    if judge_runner_invariants(extract(bad),root=root).get("decision") != "failed":
                        errors.append(f"{scenario_id}: internally balanced excess/missing UI work accepted")
                except KeyError:
                    pass
            # Exercise the real --from-report error mapping as well: missing
            # consumer CPU evidence must not be downgraded to unsupported.
            with tempfile.TemporaryDirectory(prefix="nana-transport-checker-") as tmp:
                path=Path(tmp)/"report.json"
                bad=copy.deepcopy(case);del bad["consumer_record_cpu_ms"]
                for payload, expected_status in ((bad,"error"),({**case,"status":"unsupported","evidence":"unavailable"},"unsupported")):
                    path.write_text(json.dumps(payload),encoding="utf-8")
                    result=subprocess.run([sys.executable,str(root/"perf/runners/nana/run.py"),"--repo-root",str(root),"--scenario",scenario_id,"--from-report",str(path)],cwd=root,capture_output=True,text=True,check=False)
                    try: report=json.loads(result.stdout)
                    except json.JSONDecodeError:
                        errors.append(f"{scenario_id}: runner failed to emit structured {expected_status}: {result.stderr}")
                        continue
                    if report.get("status") != expected_status or (expected_status=="error" and result.returncode==0):
                        errors.append(f"{scenario_id}: runner misclassified malformed/unavailable actual report: {report.get('status')}")
        if portable:
            for field,value in (("foreign_retirement","real"),("actual_platform_import",True)):
                bad=copy.deepcopy(case);bad["proof"][field]=value
                try:
                    extract(bad);errors.append(f"{scenario_id}: mock upgraded to platform evidence")
                except KeyError:
                    pass
        bad=copy.deepcopy(case);bad["status"]="unsupported"
        if extract(bad).get("status")!="unsupported":errors.append(f"{scenario_id}: platform unavailable treated as success")
    return errors
