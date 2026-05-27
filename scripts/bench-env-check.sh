#!/usr/bin/env bash
# Environment purity check — run before every benchmark session.
# Any FAIL = your measurement is noise. Fix env first.
set -u

driver=$(cat /sys/devices/system/cpu/cpu2/cpufreq/scaling_driver 2>/dev/null)
gov=$(cat /sys/devices/system/cpu/cpu2/cpufreq/scaling_governor 2>/dev/null)
boost=$(cat /sys/devices/system/cpu/cpufreq/boost 2>/dev/null)
hp=$(cat /proc/sys/vm/nr_hugepages 2>/dev/null)
iso=$(cat /sys/devices/system/cpu/isolated 2>/dev/null)

ok=true

echo "=== HFT Latency Lab — Environment Check ==="
echo ""

echo -n "driver(cpu2)   = $driver"
[ "$driver" = "amd-pstate-epp" ] || [ "$driver" = "acpi-cpufreq" ] && echo "  OK" || { echo "  WARN: unexpected driver"; ok=false; }

echo -n "governor(cpu2) = $gov"
[ "$gov" = "performance" ] && echo "  OK" || { echo "  FAIL (want: performance)"; ok=false; }

echo -n "boost          = $boost"
[ "$boost" = "0" ] && echo "  OK" || { echo "  FAIL (want: 0)"; ok=false; }

echo -n "hugepages      = $hp"
[ "$hp" -gt 0 ] 2>/dev/null && echo "  OK" || { echo "  WARN (want: >0)"; }

echo -n "isolated cpus  = $iso"
[ -n "$iso" ] && echo "  OK" || { echo "  WARN: no isolcpus set"; }

echo ""
if $ok; then
    echo "ENV OK — safe to benchmark."
else
    echo "ENV NOT CLEAN — fix above before measuring."
    exit 1
fi
