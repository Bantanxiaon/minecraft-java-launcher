"""Read process-scoped Windows GPU engine counters; never changes system settings."""
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import sys
import time

pid = int(sys.argv[1])
output = Path(sys.argv[2])
pdh = c.WinDLL("pdh")
query = w.HANDLE()
counter = w.HANDLE()
pdh.PdhOpenQueryW.argtypes = [w.LPCWSTR, c.c_size_t, c.POINTER(w.HANDLE)]
pdh.PdhAddEnglishCounterW.argtypes = [w.HANDLE, w.LPCWSTR, c.c_size_t, c.POINTER(w.HANDLE)]
pdh.PdhCollectQueryData.argtypes = [w.HANDLE]
pdh.PdhCloseQuery.argtypes = [w.HANDLE]
pdh.PdhGetFormattedCounterArrayW.argtypes = [w.HANDLE, w.DWORD, c.POINTER(w.DWORD), c.POINTER(w.DWORD), c.c_void_p]

class Value(c.Union):
    _fields_ = [("longValue", w.LONG), ("doubleValue", c.c_double), ("largeValue", c.c_longlong)]
class CounterValue(c.Structure):
    _fields_ = [("status", w.DWORD), ("value", Value)]
class Item(c.Structure):
    _fields_ = [("name", w.LPWSTR), ("value", CounterValue)]

samples = []
try:
    result = pdh.PdhOpenQueryW(None, 0, c.byref(query))
    if result: raise OSError("PdhOpenQueryW status " + hex(result & 0xffffffff))
    result = pdh.PdhAddEnglishCounterW(query, r"\GPU Engine(*)\Utilization Percentage", 0, c.byref(counter))
    if result: raise OSError("GPU counter unavailable: " + hex(result & 0xffffffff))
    pdh.PdhCollectQueryData(query)
    for index in range(10):
        time.sleep(1)
        pdh.PdhCollectQueryData(query)
        size, count = w.DWORD(), w.DWORD()
        pdh.PdhGetFormattedCounterArrayW(counter, 0x200, c.byref(size), c.byref(count), None)
        if not size.value: continue
        buffer = c.create_string_buffer(size.value)
        result = pdh.PdhGetFormattedCounterArrayW(counter, 0x200, c.byref(size), c.byref(count), buffer)
        if result: raise OSError("GPU sample status " + hex(result & 0xffffffff))
        items = c.cast(buffer, c.POINTER(Item))
        engines = {}
        for i in range(count.value):
            item = items[i]
            if f"pid_{pid}_" in item.name and item.value.status in (0, 1):
                engine = item.name.split("engtype_")[-1]
                engines[engine] = engines.get(engine, 0) + max(0, item.value.value.doubleValue)
        samples.append({"Sample": index, "Engines": engines})
    report = {"Measured": bool(samples) and any(x["Engines"] for x in samples),
              "Pid": pid, "Unit": "Process-scoped GPU engine utilization percent; engines are separate, not overall GPU usage",
              "Samples": samples}
except OSError as ex:
    report = {"Measured": False, "Reason": str(ex)}
finally:
    if query: pdh.PdhCloseQuery(query)
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(report, indent=2), encoding="utf-8")
print(json.dumps({"Measured": report["Measured"], "Output": str(output)}))
