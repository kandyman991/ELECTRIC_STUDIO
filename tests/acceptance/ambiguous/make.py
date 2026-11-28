# Writes the ambiguous format 1 fixture from M7's `potentials` sample (format 1, in argv[1]).
import json, sys, os
d = sys.argv[1]
P1 = "00000000-0000-0000-0000-000000000001"
P2 = "00000000-0000-0000-0000-000000000002"
def u(n): return "00000000-0000-0000-0000-%012x" % n
def page(p): return os.path.join(d, "pages", p + ".json")
devices = json.load(open(os.path.join(d, "devices.json")))
# (a) I9: an interruption whose page 1 drawing is wired to L+ and page 2 drawing to M.
devices.append({"id": u(0x1a9), "tag": "I9"})
devices.sort(key=lambda x: x["tag"])
json.dump(devices, open(os.path.join(d, "devices.json"), "w"), indent=2)
def fn(fid, dev, sym, pg, x, y, rot="R0"):
    return {"id": u(fid), "device": u(dev), "symbol": sym, "page": pg, "position": {"x": x, "y": y}, "rotation": rot}
def wire(wid, fa, fb, num):
    return {"id": u(wid), "from": {"function": u(fa), "point": "1"}, "to": {"function": u(fb), "point": "1"}, "wire_number": num}
p1 = json.load(open(page(P1)))
p2 = json.load(open(page(P2)))
p1["functions"] += [fn(0x2a1, 0x101, "iec.supply", P1, 120.0, 64.0), fn(0x2a2, 0x1a9, "iec.interruption", P1, 120.0, 40.0)]
p1["connections"] += [wire(0x3a1, 0x2a1, 0x2a2, "W91")]
p2["functions"] += [fn(0x2a3, 0x104, "iec.supply", P2, 120.0, -12.0, "R180"), fn(0x2a4, 0x1a9, "iec.interruption", P2, 120.0, 12.0, "R180")]
p2["connections"] += [wire(0x3a2, 0x2a4, 0x2a3, "W92")]
# (b) M also drawn as an interruption point, on page 2.
p2["functions"] += [fn(0x2a5, 0x104, "iec.interruption", P2, 160.0, 40.0)]
# (c) J1 drawn a second time, on page 2.
p2["functions"] += [fn(0x2a6, 0x109, "iec.junction", P2, 160.0, -40.0)]
json.dump(p1, open(page(P1), "w"), indent=2)
json.dump(p2, open(page(P2), "w"), indent=2)
