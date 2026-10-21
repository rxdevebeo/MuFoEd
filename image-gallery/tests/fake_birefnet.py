"""A stand-in for birefnet_worker.py in the gallery's tests: the same
protocol, plain Python, no model. The "mask" keeps pixels whose red channel
is above the picture's average; a model file reading "broken" fails like a
bad graph."""
import argparse
import json
import struct
import sys

SIDE = 64


def say(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode")
    parser.add_argument("--model", required=True)
    parser.add_argument("--device", default="cpu")
    args = parser.parse_args()
    with open(args.model, "rb") as stream:
        if stream.read() == b"broken":
            say({"error": "broken model"})
            return 1
    say({"ready": True, "width": SIDE, "height": SIDE, "provider": "Fake"})
    plane = SIDE * SIDE
    for line in sys.stdin:
        if not line.strip():
            continue
        request = json.loads(line)
        with open(request["input"], "rb") as stream:
            data = stream.read()
        if len(data) != 3 * plane * 4:
            say({"error": "input tensor of the wrong size"})
            continue
        red = struct.unpack("<%df" % plane, data[: plane * 4])
        average = sum(red) / plane
        logits = [40.0 if value > average else -40.0 for value in red]
        with open(request["output"], "wb") as stream:
            stream.write(struct.pack("<%df" % plane, *logits))
        say({"ok": True})
    return 0


if __name__ == "__main__":
    sys.exit(main())
