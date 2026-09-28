$lines = Get-Content "c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_REPORT.md" -Encoding UTF8
$tableRows = 0
$datedRows = 0

for ($i = 0; $i -lt $lines.Count; $i++) {
    $line = $lines[$i]
    if ($line -match "^\|\s*\d+\s*\|") {
        $tableRows++
        if ($line -match "2026-09-28") {
            $datedRows++
        } else {
            Write-Output "Undated row $($i+1): $line"
        }
    }
}

Write-Output "Total numbered table rows: $tableRows, dated with 2026-09-28: $datedRows"
