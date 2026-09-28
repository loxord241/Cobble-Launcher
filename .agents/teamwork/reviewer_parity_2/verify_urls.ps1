$lines = Get-Content "c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_REPORT.md" -Encoding UTF8
$lineNum = 0
$issues = @()

for ($i = 0; $i -lt $lines.Count; $i++) {
    $lineNum = $i + 1
    $line = $lines[$i]
    if ($line -match "https?://" -or $line -match "[a-zA-Z0-9-]+\.(org|com|net|io|gs|by)") {
        if ($line -notmatch "2026-09-28") {
            $issues += "Line $lineNum : $line"
        }
    }
}

Write-Output "Found $($issues.Count) candidate lines without date:"
$issues | ForEach-Object { Write-Output $_ }
