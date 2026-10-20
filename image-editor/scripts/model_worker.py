"""Owned persistent worker. Each request finishes with an atomic JSON acknowledgement."""
import argparse
import contextlib
import importlib
import json
from pathlib import Path
import sys
import traceback
import worker_cache


def main():
    modules = {"birefnet.py": "birefnet", "sam2_tool.py": "sam2_tool", "upscale_tool.py": "upscale_tool"}
    for line in sys.stdin:
        request = json.loads(line)
        response = Path(request["response"])
        worker_cache.PHASE_FILE = Path(request["phase_file"]) if "phase_file" in request else None
        result = {}
        try:
            with contextlib.redirect_stdout(sys.stderr):
                if request["action"] == "offload":
                    result["offloaded"] = worker_cache.offload()
                elif request["action"] == "run":
                    module = importlib.import_module(modules[request["script"]])
                    args = module.parse_args(request["args"])
                    function = {"remove": module.remove_background} if request["script"] == "birefnet.py" else {"segment": module.segment} if request["script"] == "sam2_tool.py" else {"upscale": module.upscale}
                    function[args.action](args)
                else:
                    raise ValueError("Unknown worker action")
            result.update(worker_cache.metrics())
            result["ok"] = True
        except Exception as error:
            result = {"ok": False, "error": str(error), "trace": traceback.format_exc()}
        temporary = response.with_suffix(".pending")
        temporary.write_text(json.dumps(result), encoding="utf-8")
        temporary.replace(response)


if __name__ == "__main__":
    main()
