# Saved as UTF-8 with a byte order mark, so Windows PowerShell 5.1 reads the Kurmanji text right.
# Registers the SAPI5 voice and speaks through SAPI's own SpVoice, the way Narrator's legacy
# voices and NVDA's SAPI5 driver reach it. Run it with the PowerShell whose bitness matches the
# DLL: SysWOW64's for the 32-bit build.
#   smoke.ps1 -Stage <dir with dengjen_sapi.dll, onnxruntime.dll, espeak-ng-data, voices>
param(
    [Parameter(Mandatory)][string]$Stage,
    [string]$Out = (Join-Path $env:TEMP 'dengjen-sapi-smoke.wav')
)
$ErrorActionPreference = 'Stop'

$dll = Join-Path $Stage 'dengjen_sapi.dll'
$is32 = [IntPtr]::Size -eq 4
$regsvr32 = if ($is32 -and [Environment]::Is64BitOperatingSystem) {
    "$env:WINDIR\SysWOW64\regsvr32.exe"
} else {
    "$env:WINDIR\System32\regsvr32.exe"
}

function Invoke-Regsvr32([string[]]$Arguments) {
    $process = Start-Process -FilePath $regsvr32 -ArgumentList ($Arguments + "`"$dll`"") -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "regsvr32 $Arguments failed with exit code $($process.ExitCode)"
    }
}

function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAILED: $Message" }
    Write-Host "ok: $Message"
}

# SAPI's flags, from sapi.h.
$SVSFlagsAsync = 1
$SVSFPurgeBeforeSpeak = 2
$SVSFIsXML = 8
$SSFMCreateForWrite = 3
$SVEAllEvents = 33790

function New-DengjenVoice {
    $voice = New-Object -ComObject SAPI.SpVoice
    $token = @($voice.GetVoices() | Where-Object { $_.GetDescription() -like 'Dengjen*' })[0]
    if ($null -eq $token) { throw 'no Dengjen voice is registered' }
    $voice.Voice = $token
    $voice.EventInterests = $SVEAllEvents
    $voice
}

function Open-Wave([object]$Voice, [string]$Path) {
    $stream = New-Object -ComObject SAPI.SpFileStream
    $stream.Open($Path, $SSFMCreateForWrite, $false)
    $Voice.AudioOutputStream = $stream
    $stream
}

Write-Host "Testing $dll as a $(if ($is32) { '32' } else { '64' })-bit voice"

# Per-user registration needs no admin rights and must clean up after itself.
Invoke-Regsvr32 @('/s', '/n', '/i:user')
$userTokens = 'HKCU:\SOFTWARE\Microsoft\Speech\Voices\Tokens'
Assert (@(Get-ChildItem $userTokens | Where-Object { $_.PSChildName -like 'Dengjen-*' }).Count -gt 0) 'user registration adds a token'
Invoke-Regsvr32 @('/s', '/u', '/n', '/i:user')
Assert (@(Get-ChildItem $userTokens -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -like 'Dengjen-*' }).Count -eq 0) 'user unregistration removes it'

Invoke-Regsvr32 @('/s')
$voice = New-DengjenVoice
Assert ($voice.Voice.GetDescription() -like 'Dengjen*') "SAPI lists $($voice.Voice.GetDescription())"
Assert ($voice.Voice.GetAttribute('Language') -ne '') "the voice declares Language=$($voice.Voice.GetAttribute('Language'))"

# SpVoice raises its events through window messages on this thread, so the wait pumps them.
# PowerShell cannot subscribe to a late-bound COM object's events, so a small class hooks
# _ISpeechVoiceEvents by dispatch id: 4 is Bookmark, 5 is Word.
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public class SapiEvents : IDisposable
{
    static readonly Guid Events = new Guid("A372ACD1-3BEF-4BBD-8FFB-CB3E2B416AF8");
    delegate void Handler(object streamNumber, object streamPosition, object third, object fourth);
    public readonly List<string> Bookmarks = new List<string>();
    public readonly List<int> Words = new List<int>();
    readonly object voice;
    readonly Handler bookmark;
    readonly Handler word;

    public SapiEvents(object voice)
    {
        this.voice = voice;
        bookmark = (number, position, name, id) => Bookmarks.Add(name + "#" + id);
        word = (number, position, character, length) => Words.Add(Convert.ToInt32(character));
        ComEventsHelper.Combine(voice, Events, 4, bookmark);
        ComEventsHelper.Combine(voice, Events, 5, word);
    }

    public void Dispose()
    {
        ComEventsHelper.Remove(voice, Events, 4, bookmark);
        ComEventsHelper.Remove(voice, Events, 5, word);
    }
}
'@

function Invoke-Speech([object]$Voice, [string]$Text, [string]$Path) {
    $stream = Open-Wave $Voice $Path
    $events = New-Object SapiEvents -ArgumentList $Voice.PSObject.BaseObject
    $watch = [Diagnostics.Stopwatch]::StartNew()
    [void]$Voice.Speak($Text, $SVSFIsXML + $SVSFlagsAsync)
    while (-not $Voice.WaitUntilDone(20)) { [Windows.Forms.Application]::DoEvents() }
    foreach ($i in 1..10) { [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 }
    $watch.Stop()
    $stream.Close()
    $events.Dispose()
    [pscustomobject]@{
        Bytes = (Get-Item $Path).Length
        Ms = $watch.ElapsedMilliseconds
        Bookmarks = @($events.Bookmarks)
        Words = @($events.Words)
    }
}

# A built-in voice through the same probe, so a broken probe is told apart from a broken engine.
$control = New-Object -ComObject SAPI.SpVoice
$builtin = @($control.GetVoices() | Where-Object { $_.GetDescription() -notlike 'Dengjen*' })[0]
if ($null -ne $builtin) {
    $control.Voice = $builtin
    $control.EventInterests = $SVEAllEvents
    $c = Invoke-Speech $control 'Hello there. <bookmark mark="7"/>How are you?' (Join-Path $env:TEMP 'dengjen-sapi-control.wav')
    Write-Host "control ($($builtin.GetDescription())): bookmarks=[$($c.Bookmarks -join ',')] words=[$($c.Words -join ',')]"
}

# Speech, a bookmark and word events, written to a file: CI runners have no sound card.
$spoken = Invoke-Speech $voice 'Silav, tu çawa yî? <bookmark mark="7"/>Ez baş im, spas.' $Out
Write-Host "spoke $($spoken.Bytes) bytes in $($spoken.Ms) ms: bookmarks=[$($spoken.Bookmarks -join ',')] words=[$($spoken.Words -join ',')]"
Assert ($spoken.Bytes -gt 44 + 32000) 'the voice wrote at least a second of audio'
Assert ($spoken.Bookmarks -contains '7#7') 'the bookmark fired with its name and number'
Assert ($spoken.Words.Count -ge 6) "word events fired ($($spoken.Words.Count))"
Assert (($spoken.Words | Select-Object -Last 1) -gt 0) 'word events report positions in the input'

# A purge must stop a long request quickly: screen readers purge on every key press.
$voice = New-DengjenVoice
$stream = Open-Wave $voice (Join-Path $env:TEMP 'dengjen-sapi-abort.wav')
$long = (1..40 | ForEach-Object { "Ev hevoka hejmar $_ e, û ew gelekî dirêj e da ku axaftin demekê bidome." }) -join ' '
[void]$voice.Speak($long, $SVSFlagsAsync)
Start-Sleep -Milliseconds 1500
$watch = [Diagnostics.Stopwatch]::StartNew()
[void]$voice.Speak('', $SVSFPurgeBeforeSpeak)
$done = $voice.WaitUntilDone(10000)
$watch.Stop()
$stream.Close()
Write-Host "purge finished in $($watch.ElapsedMilliseconds) ms"
Assert $done 'the purge finished'
Assert ($watch.ElapsedMilliseconds -lt 2000) 'the purge stopped within two seconds'

Invoke-Regsvr32 @('/s', '/u')
$left = @((New-Object -ComObject SAPI.SpVoice).GetVoices() | Where-Object { $_.GetDescription() -like 'Dengjen*' })
Assert ($left.Count -eq 0) 'unregistering removes the voice'
