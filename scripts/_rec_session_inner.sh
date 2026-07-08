#!/usr/bin/env bash
# Inner session actually executed inside the asciinema PTY (a real interactive-ish shell).
set -u
export NO_ANIM=1
cd /root/bebop-repo
B="npx tsx bebop.ts"

echo "### Bebop — real CLI, recorded live"
sleep 1
echo "\$ bebop boot"
$B boot
sleep 2
echo ""
echo "\$ bebop status"
$B status
sleep 2
echo ""
echo "\$ bebop use native"
$B use native
sleep 1.5
echo ""
echo "\$ bebop dispatch \"add a tinyhealth check to the guard\""
$B dispatch "add a tiny health check to the guard" 2>&1 | head -20
sleep 2.5
echo ""
echo "\$ bebop route reason"
$B route reason
sleep 1.5
echo ""
echo "\$ bebop map"
$B map 2>&1 | head -5
sleep 1.5
echo ""
echo "### recorded with asciinema — real output, no faking"
sleep 1
