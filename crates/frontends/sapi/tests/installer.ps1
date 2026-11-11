# Saved as UTF-8 with a byte order mark, so Windows PowerShell 5.1 reads the Kurmanji text right.
# Runs the SAPI5 voice installer silently, checks that SAPI lists the voice machine-wide and that
# it speaks, then uninstalls and checks that the voice, its COM class and its files are gone.
#   installer.ps1 -Setup <dengjen-sapi-...-setup.exe>
param(
    [Parameter(Mandatory)][string]$Setup
)
$ErrorActionPreference = 'Stop'

function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAILED: $Message" }
    Write-Host "ok: $Message"
}

function Get-DengjenTokens {
    @((New-Object -ComObject SAPI.SpVoice).GetVoices() | Where-Object { $_.GetDescription() -like 'Dengjen*' })
}

function Invoke-Silently([string]$Path, [string]$Log) {
    $process = Start-Process -FilePath $Path -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$Log`"") -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        Get-Content $Log -ErrorAction SilentlyContinue | Select-Object -Last 40 | Write-Host
        throw "$Path failed with exit code $($process.ExitCode)"
    }
}

$app = Join-Path $env:ProgramFiles 'Dengjen\SAPI5 voice'
$machineTokens = 'HKLM:\SOFTWARE\Microsoft\Speech\Voices\Tokens'
$class = 'HKLM:\SOFTWARE\Classes\CLSID\{EE94E0DE-50A8-4645-B76B-38F65AE052D4}'

Invoke-Silently $Setup (Join-Path $env:TEMP 'dengjen-sapi-install.log')
Assert (Test-Path (Join-Path $app 'dengjen_sapi.dll')) "the DLL is installed in $app"
Assert (@(Get-ChildItem $machineTokens | Where-Object { $_.PSChildName -like 'Dengjen-*' }).Count -gt 0) 'the voice is registered machine-wide'
# A function returning one item hands back the item itself, so wrap it to index it.
$tokens = @(Get-DengjenTokens)
Assert ($tokens.Count -gt 0) "SAPI lists $($tokens[0].GetDescription())"
$voicePath = (Get-ItemProperty (Join-Path $machineTokens $tokens[0].Id.Split('\')[-1])).VoicePath
Assert ($voicePath -like "$app*") "the token points into the install directory ($voicePath)"

# Speaking loads the DLL, which then stays loaded for the life of the process, so it happens in a
# child process that has exited by the time the uninstaller deletes the file.
$bytes = Start-Job -ScriptBlock {
    $voice = New-Object -ComObject SAPI.SpVoice
    $voice.Voice = @($voice.GetVoices() | Where-Object { $_.GetDescription() -like 'Dengjen*' })[0]
    $stream = New-Object -ComObject SAPI.SpMemoryStream
    $voice.AudioOutputStream = $stream
    [void]$voice.Speak("Rojba$([char]0x15F), ez deng$([char]0xEA) Dengjen im.", 0)
    ([byte[]]$stream.GetData()).Length
} | Wait-Job | Receive-Job
Assert ($bytes -gt 16000) "the installed voice speaks ($bytes bytes)"

$uninstaller = Join-Path $app 'unins000.exe'
Assert (Test-Path $uninstaller) 'the uninstaller is installed'
Invoke-Silently $uninstaller (Join-Path $env:TEMP 'dengjen-sapi-uninstall.log')
# The uninstaller copies itself to a temporary folder and returns before that copy finishes.
$deadline = (Get-Date).AddSeconds(60)
while ((Test-Path $uninstaller) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
Assert (@(Get-DengjenTokens).Count -eq 0) 'uninstalling removes the voice from SAPI'
Assert (-not (Test-Path $class)) 'uninstalling removes the COM class'
Assert (-not (Test-Path (Join-Path $app 'dengjen_sapi.dll'))) 'uninstalling removes the DLL'
Assert (-not (Test-Path (Join-Path $app 'voices'))) 'uninstalling removes the voices'
