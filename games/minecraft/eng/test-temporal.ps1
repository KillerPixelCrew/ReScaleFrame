param([string]$JavaHome='C:\Users\N1GHT\AppData\Roaming\PrismLauncher\java\java-runtime-delta')
$ErrorActionPreference='Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    & .\gradlew.bat "-Dorg.gradle.java.installations.paths=$($JavaHome.Replace('\','/'))" test jar
    if($LASTEXITCODE -ne 0) { throw "Temporal harness failed ($LASTEXITCODE)." }
} finally { Pop-Location }
