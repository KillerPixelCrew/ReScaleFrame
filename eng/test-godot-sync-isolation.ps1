# Process-only synchronization-validation control. No RTSS/profile settings change.
param(
    [Parameter(Mandatory)][string]$EngineExe,
    [Parameter(Mandatory)][string]$ValidationLayerDirectory,
    [ValidateSet('normal','extension-points-disabled')][string[]]$Mode = @('normal','extension-points-disabled'),
    [ValidateRange(30,240)][int]$Frames = 120
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
. (Join-Path $PSScriptRoot 'godot-process-isolation.ps1')
$taskExe = (Resolve-Path -LiteralPath $EngineExe).Path
$taskLayer = (Resolve-Path -LiteralPath $ValidationLayerDirectory).Path
if (!(Test-Path -LiteralPath (Join-Path $taskLayer 'VkLayer_khronos_validation.json'))) { throw 'Portable Khronos layer manifest is missing.' }
$taskStamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$taskDirectory = Join-Path $taskRoot ".local/godot/sync-controls/$taskStamp"
$taskProject = Join-Path $taskDirectory 'project'
New-Item -ItemType Directory -Force -Path $taskProject | Out-Null
@'
config_version=5
[application]
config/name="ReScaleFrame synchronization control"
run/main_scene="res://main.tscn"
[display]
window/size/viewport_width=640
window/size/viewport_height=360
[rendering]
renderer/rendering_method="gl_compatibility"
'@ | Set-Content -LiteralPath (Join-Path $taskProject 'project.godot') -Encoding utf8
@'
[gd_scene load_steps=2 format=3]
[ext_resource type="Script" path="res://main.gd" id="1"]
[node name="SyncControl" type="Node3D"]
script=ExtResource("1")
'@ | Set-Content -LiteralPath (Join-Path $taskProject 'main.tscn') -Encoding utf8
@'
extends Node3D
var frame := 0
var last_ticks := 0
var samples: Array[int] = []
var result_path := ""
var target_frames := 120
var box: MeshInstance3D
func _ready() -> void:
    Engine.max_fps = 60
    for arg: String in OS.get_cmdline_user_args():
        if arg.begins_with("--result="): result_path = arg.trim_prefix("--result=")
        if arg.begins_with("--frames="): target_frames = arg.trim_prefix("--frames=").to_int()
    var camera := Camera3D.new()
    add_child(camera)
    camera.position = Vector3(0, 2, 6)
    camera.look_at(Vector3.ZERO)
    camera.current = true
    box = MeshInstance3D.new()
    box.mesh = BoxMesh.new()
    var material := StandardMaterial3D.new()
    material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
    material.albedo_color = Color(0.1, 0.65, 0.9)
    box.material_override = material
    add_child(box)
    last_ticks = Time.get_ticks_usec()
func _process(_delta: float) -> void:
    frame += 1
    var now := Time.get_ticks_usec()
    samples.append(now - last_ticks)
    last_ticks = now
    box.position.x = sin(float(frame) / 20.0) * 1.5
    if frame >= target_frames:
        var file := FileAccess.open(result_path, FileAccess.WRITE)
        file.store_string(JSON.stringify({"frames":frame,"cpu_intervals_us":samples,"driver":RenderingServer.get_current_rendering_driver_name()}, "  "))
        get_tree().quit()
'@ | Set-Content -LiteralPath (Join-Path $taskProject 'main.gd') -Encoding utf8
$taskSaved = @{}
foreach ($taskKey in @('VK_LAYER_PATH','VK_LOADER_LAYERS_DISABLE','VK_VALIDATION_VALIDATE_SYNC','VK_VALIDATION_REPORT_FLAGS')) {
    $taskSaved[$taskKey] = [Environment]::GetEnvironmentVariable($taskKey)
}
$taskCases = @()
try {
    $env:VK_LAYER_PATH = $taskLayer
    $env:VK_LOADER_LAYERS_DISABLE = '~implicit~'
    $env:VK_VALIDATION_VALIDATE_SYNC = '1'
    $env:VK_VALIDATION_REPORT_FLAGS = 'error,warn,info'
    foreach ($taskMode in $Mode) {
        $taskLease = Enter-RsfGodotValidation
        try {
            $taskOutput = Join-Path $taskDirectory $taskMode
            New-Item -ItemType Directory -Force -Path $taskOutput | Out-Null
            $taskResult = Join-Path $taskOutput 'metrics.json'
            $taskArgs = @('--path',$taskProject,'--rendering-method','forward_plus','--rendering-driver','vulkan','--position','100,100','--gpu-validation','--',"--result=$taskResult","--frames=$Frames")
            $taskArguments = ($taskArgs | ForEach-Object { if ($_ -match '["\r\n]') { throw 'Invalid process argument.' }; '"'+$_+'"' }) -join ' '
            [uint32]$taskPolicy = 0
            $taskTimer = [Diagnostics.Stopwatch]::StartNew()
            $taskProcess = [RsfSyncIsolationProcess]::Start($taskExe,$taskArguments,$taskProject,(Join-Path $taskOutput 'stdout.log'),(Join-Path $taskOutput 'stderr.log'),($taskMode -eq 'extension-points-disabled'),[ref]$taskPolicy)
            $taskModules = @{}
            try {
                Set-RsfGodotForeground $taskProcess
                while (!$taskProcess.HasExited) {
                    foreach ($taskModule in $taskProcess.Modules) {
                        if (!$taskModules.ContainsKey($taskModule.FileName)) {
                            $taskModules[$taskModule.FileName] = @{name=$taskModule.ModuleName; path=$taskModule.FileName; first_seen_ms=$taskTimer.ElapsedMilliseconds}
                        }
                    }
                    if ($taskProcess.WaitForExit(100)) { break }
                    if ($taskTimer.Elapsed.TotalSeconds -gt 60) { Stop-Process -Id $taskProcess.Id; throw 'Owned synchronization control exceeded 60 seconds.' }
                    $taskProcess.Refresh()
                }
                $taskExit = $taskProcess.ExitCode
            } catch { if (!$taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id }; throw }
            foreach ($taskModule in $taskModules.Values) {
                if ($taskModule.name -match '(?i)RTSS|VkLayer_khronos|nvoglv|rescaleframe|streamline') { $taskModule.sha256 = (Get-FileHash -LiteralPath $taskModule.path).Hash }
            }
            $taskLog = (Get-Content -Raw -LiteralPath (Join-Path $taskOutput 'stdout.log')) + (Get-Content -Raw -LiteralPath (Join-Path $taskOutput 'stderr.log'))
            $taskCase = @{mode=$taskMode; requested_extension_disable=($taskMode -eq 'extension-points-disabled'); effective_extension_policy=$taskPolicy;
                process_id=$taskProcess.Id; exit_code=$taskExit; duration_ms=$taskTimer.ElapsedMilliseconds; engine=$taskExe; engine_sha256=(Get-FileHash -LiteralPath $taskExe).Hash;
                synchronization_enabled=($taskLog -match '(?m)^\s*- Synchronization\s*$'); present_after_write_count=[regex]::Matches($taskLog,'PRESENT_AFTER_WRITE').Count;
                vuid_ids=@([regex]::Matches($taskLog,'VUID-[A-Za-z0-9_-]+') | ForEach-Object Value | Sort-Object -Unique);
                modules=@($taskModules.Values | Sort-Object name); metrics=$taskResult; output=$taskOutput}
            $taskCase | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $taskOutput 'receipt.json') -Encoding utf8
            $taskCases += $taskCase
            Write-Output "$taskMode : PRESENT_AFTER_WRITE=$($taskCase.present_after_write_count), effective policy=$taskPolicy, modules=$($taskModules.Count)"
        } finally { $taskLease.ReleaseMutex(); $taskLease.Dispose() }
    }
} finally { Restore-RsfGodotEnvironment $taskSaved }
@{scope='Process-only official/native Vulkan WSI control; no add-on, profile change or image dump'; cases=$taskCases} |
    ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $taskDirectory 'summary.json') -Encoding utf8
Write-Output "Synchronization control receipts: $taskDirectory"
