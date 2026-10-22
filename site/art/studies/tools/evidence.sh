#!/bin/sh
# Capture the agent-loop evidence for the sprout study. Run from site/art/studies.
# usage: ARTSCRIPT=/path/to/artscript sh tools/evidence.sh
# Resets the luna ramp to its moth-lib hex, records the failing check and the passing build,
# then changes that one palette line to the recolor hex and records the build again.
set -u
A=${ARTSCRIPT:-artscript}
BEFORE='#b9cfae'
AFTER='#b8a8d6'
mkdir -p evidence
sed -i '' "s/ramp luna $AFTER 7/ramp luna $BEFORE 7/" art/sprout.art

$A check --json art/sprout_broken.art > evidence/check.json
printf 'exit=%s\n' $? > evidence/check.exit
$A build --only 'art/sprout_broken.art' > /dev/null 2>&1

$A build --only 'art/sprout.art'
python3 -c 'import json,sys;m=json.load(open("assets/art-manifest.json"));json.dump({"outputs":[o for o in m["outputs"] if o["path"]=="sprout.png"]},open(sys.argv[1],"w"),indent=2,sort_keys=True)' evidence/manifest_before.json
cp assets/sprout.png evidence/sprout.png
cp art/sprout.art evidence/.sprout_before.art

sed -i '' "s/ramp luna $BEFORE 7/ramp luna $AFTER 7/" art/sprout.art
diff -U0 --label a/art/sprout.art --label b/art/sprout.art evidence/.sprout_before.art art/sprout.art > evidence/diff.txt
rm evidence/.sprout_before.art
$A build --only 'art/sprout.art'
python3 -c 'import json,sys;m=json.load(open("assets/art-manifest.json"));json.dump({"outputs":[o for o in m["outputs"] if o["path"]=="sprout.png"]},open(sys.argv[1],"w"),indent=2,sort_keys=True)' evidence/manifest_after.json
cp assets/sprout.png evidence/sprout_recolor.png
$A explain art/sprout.art | head -25 > evidence/explain.txt
