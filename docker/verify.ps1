<#
.SYNOPSIS
    Crystal Studio —— Docker 部署「一条命令」验证（Windows / PowerShell 版）

.DESCRIPTION
    构建 → 起 → 轮询 /api/health → 四项端点断言 → 收工。
    成功时**只认这一行**：   DOCKER_VERIFY: PASS
    失败时打印原因并以退出码 1 结束。

    验的 5 项（全绿才算 PASS）：
      1. GET  /api/health            → 200 且 body 含 "ok":true
      2. POST /api/map/open          → 200 且 body 含 "width"
      3. GET  /api/tiles             → 200，PNG 魔数，字节数 > 1000
      4. GET  /api/stamps?q=market   → 200 且 total > 0
      5. GET  /                       → 200 且 Content-Type 是 text/html

    脚本不读源码：只驱动 `docker compose` + `curl.exe` 打已发布端口。
    所以把本文件单独拷到一台只有 compose.yaml 的机器上，配 -SkipBuild 也能验。

.PARAMETER Dir
    compose 项目目录（默认：本脚本所在目录）

.PARAMETER File
    compose 文件名（默认 compose.yaml）

.PARAMETER Service
    服务名（默认 miredit）

.PARAMETER Port
    宿主端口（默认读 $env:MIREDIT_PORT，再默认 8080）

.PARAMETER Timeout
    健康轮询上限秒数（默认 60）

.PARAMETER Map
    /data/maps 下要打开的地图文件名（默认自动找第一个 .map）

.PARAMETER Keep
    验证结束后不停容器

.PARAMETER SkipBuild
    跳过 docker compose build（镜像已构建好时用）

.EXAMPLE
    .\verify.ps1

.EXAMPLE
    .\verify.ps1 -Keep -Port 18080
#>
[CmdletBinding()]
param(
    [string]$Dir,
    [string]$File = 'compose.yaml',
    [string]$Service = 'miredit',
    [int]$Port = 0,
    [int]$Timeout = 60,
    [string]$Map,
    [switch]$Keep,
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------- 默认参数 ---
if (-not $Dir) { $Dir = $PSScriptRoot }
if (-not $Dir) { $Dir = (Get-Location).Path }
if ($Port -le 0) {
    if ($env:MIREDIT_PORT) { $Port = [int]$env:MIREDIT_PORT } else { $Port = 8080 }
}
$TileW = if ($env:MIREDIT_TILE_W) { [int]$env:MIREDIT_TILE_W } else { 16 }
$TileH = if ($env:MIREDIT_TILE_H) { [int]$env:MIREDIT_TILE_H } else { 16 }
if (-not $Map) { $Map = $env:MIREDIT_MAP_FILE }

$Base = "http://127.0.0.1:$Port"

# ------------------------------------------------------------------ 输出 -----
function Write-Say  { param([string]$m) Write-Host $m }
function Write-Ok   { param([string]$m) Write-Host "[ OK ] $m" -ForegroundColor Green }
function Write-Bad  { param([string]$m) Write-Host "[FAIL] $m" -ForegroundColor Red }
function Write-Warn2{ param([string]$m) Write-Host "[WARN] $m" -ForegroundColor Yellow }
function Stop-Fail  {
    param([string]$m)
    Write-Bad $m
    Write-Say ''
    Write-Say 'DOCKER_VERIFY: FAIL'
    exit 1
}

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("miredit-verify-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Tmp -Force | Out-Null

# --------------------------------------------------------- 0. 前置检查 -------
$docker = Get-Command docker -ErrorAction SilentlyContinue
if (-not $docker) { Stop-Fail '找不到 docker（本机没装 Docker？）' }
$composeOk = $true
try { & docker compose version *> $null } catch { $composeOk = $false }
if ($LASTEXITCODE -ne 0 -or -not $composeOk) { Stop-Fail 'docker compose v2 不可用（试 `docker compose version`）' }
$curl = Get-Command curl.exe -ErrorAction SilentlyContinue
if (-not $curl) { Stop-Fail '找不到 curl.exe（Windows 10 1803+ 自带；本脚本用它打端点）' }
if (-not (Test-Path -LiteralPath $Dir -PathType Container)) { Stop-Fail "目录不存在：$Dir" }
$ComposeFile = Join-Path $Dir $File
if (-not (Test-Path -LiteralPath $ComposeFile -PathType Leaf)) { Stop-Fail "找不到 compose 文件：$ComposeFile" }
Push-Location $Dir

# 统一调用 curl.exe 的包装：返回 @{Code; BodyFile}
function Invoke-Curl {
    param(
        [string]$Url,
        [string]$OutFile,
        [string]$Method = 'GET',
        [string]$BodyFile,
        [int]$MaxTime = 30
    )
    $argv = @('-sS', '--max-time', "$MaxTime", '-o', $OutFile, '-w', '%{http_code}', '-X', $Method)
    if ($BodyFile) { $argv += @('-H', 'content-type: application/json', '--data-binary', "@$BodyFile") }
    $argv += $Url
    $code = & curl.exe @argv 2>$null
    if ($LASTEXITCODE -ne 0 -and -not $code) { $code = '000' }
    return ($code | Select-Object -Last 1)
}

function Read-Text {
    param([string]$Path)
    if (Test-Path -LiteralPath $Path) { return (Get-Content -LiteralPath $Path -Raw -ErrorAction SilentlyContinue) }
    return ''
}

Write-Say '=============================================================='
Write-Say ' Crystal Studio · Docker 部署验证（PowerShell）'
Write-Say " 项目目录 : $Dir"
Write-Say " compose  : $File（服务 $Service）"
Write-Say " 访问地址 : $Base"
Write-Say '=============================================================='

try {
    # ----------------------------------------------------------- 1. 构建 -----
    if ($SkipBuild) {
        Write-Say '[1/5] 跳过构建（-SkipBuild）'
    }
    else {
        Write-Say '[1/5] docker compose build ...'
        & docker compose -f $File build
        if ($LASTEXITCODE -ne 0) {
            Stop-Fail '构建失败。常见原因：构建上下文不是 tools/（见 docker/README.md §6）、或 Cargo.lock 与清单不同步（--locked）'
        }
        Write-Ok '构建完成'
    }

    # ------------------------------------------------------------- 2. 起 -----
    Write-Say '[2/5] docker compose up -d ...'
    & docker compose -f $File up -d
    if ($LASTEXITCODE -ne 0) {
        Stop-Fail "启动失败。看日志：docker compose -f $ComposeFile logs --tail=100 $Service"
    }
    Write-Ok '容器已启动'

    # --------------------------------------------- 3. 轮询 /api/health -------
    Write-Say "[3/5] 轮询 $Base/api/health（上限 ${Timeout}s）..."
    $healthFile = Join-Path $Tmp 'health.json'
    $deadline = (Get-Date).AddSeconds($Timeout)
    $healthy = $false
    $lastBody = ''
    while ((Get-Date) -lt $deadline) {
        $code = Invoke-Curl -Url "$Base/api/health" -OutFile $healthFile -MaxTime 3
        if ($code -eq '200') {
            $lastBody = Read-Text $healthFile
            if ($lastBody -match '"ok"\s*:\s*true') { $healthy = $true; break }
        }
        Start-Sleep -Seconds 1
    }

    if (-not $healthy) {
        Write-Bad "健康检查在 ${Timeout}s 内没通过（最后响应：$(if ($lastBody) { $lastBody } else { '<无>' })）"
        Write-Say ''
        Write-Say '---- 容器状态 ----'
        & docker compose -f $File ps
        Write-Say ''
        Write-Say '---- 最近 60 行日志 ----'
        & docker compose -f $File logs --tail=60 $Service
        Write-Say ''
        Write-Say '诊断提示：'
        Write-Say '  * 端口被占？  改 -Port，或用 $env:MIREDIT_PORT=18080 重跑'
        Write-Say '  * 进程起不来？ 上面日志里应有 [启动失败] 一行'
        Write-Say "  * 想看现场？  加 -Keep 重跑，然后 docker compose logs -f $Service"
        Stop-Fail '健康检查未通过'
    }
    Write-Ok "/api/health 200 → $lastBody"

    # ------------------------------------ 4. 找出要打开的地图文件 -----------
    if (-not $Map) {
        # 容器里只做 `ls`，取第一条在宿主侧做（管道/重定向都不进容器命令：
        # 少一层 shell 语义，也避免 Windows 上 cmd 把 `|` 当管道重新解析）
        $raw = & docker compose -f $File exec -T $Service sh -c 'ls /data/maps/*.map' 2>$null
        if ($LASTEXITCODE -eq 0 -and $raw) {
            $line = ($raw | Where-Object { $_ -and $_.Trim() } | Select-Object -First 1)
            if ($line) { $Map = ($line.Trim() -split '/')[-1] }
        }
    }
    if (-not $Map) {
        $Map = '0.map'
        Write-Warn2 "没在 /data/maps 下找到 .map，回退试 $Map"
    }
    Write-Say "      将打开地图：$Map"

    # --------------------------------------------- 5. 四项端点断言 -----------
    Write-Say '[4/5] 断言端点 ...'

    # ---- ① POST /api/map/open ------------------------------------------
    $openBody = Join-Path $Tmp 'open-body.json'
    # ConvertTo-Json 负责转义（文件名里的 \ 与 " 都不会破坏 JSON）
    [System.IO.File]::WriteAllText($openBody, (@{ path = $Map } | ConvertTo-Json -Compress), [System.Text.UTF8Encoding]::new($false))
    $openOut = Join-Path $Tmp 'open.json'
    $code = Invoke-Curl -Url "$Base/api/map/open" -OutFile $openOut -Method 'POST' -BodyFile $openBody
    $body = Read-Text $openOut
    if ($code -ne '200') {
        Write-Bad "POST /api/map/open → HTTP $code"
        Write-Say "      $body"
        Write-Say "      诊断：/data/maps 里没有 $Map？用 -Map <文件名> 指定；"
        Write-Say '            或 MIREDIT_MAP_DIR 指向正确的地图目录（见 docker/README.md §5）'
        Stop-Fail 'map/open 未通过'
    }
    if ($body -notmatch '"width"') { Stop-Fail "POST /api/map/open 200 但 body 里没有 width：$body" }
    Write-Ok ("POST /api/map/open 200 且含 width → " + $body.Substring(0, [Math]::Min(160, $body.Length)) + '...')

    # ---- ② GET /api/tiles ----------------------------------------------
    $tiles = Join-Path $Tmp 'tiles.png'
    $code = Invoke-Curl -Url "$Base/api/tiles?x=0&y=0&w=$TileW&h=$TileH" -OutFile $tiles -MaxTime 60
    if ($code -ne '200') {
        Write-Bad "GET /api/tiles → HTTP $code"
        Write-Say "      $(Read-Text $tiles)"
        Write-Say '      诊断：503 通常是图库没挂上 —— MIREDIT_CLIENT_DIR 要指向'
        Write-Say '            「含 .wil/.wzl 的客户端根」（本仓 = client/热血传奇23周年客户端）'
        Stop-Fail 'tiles 未通过'
    }
    $size = (Get-Item -LiteralPath $tiles).Length
    # PNG 魔数：89 50 4E 47 0D 0A 1A 0A
    $head = New-Object byte[] 8
    $fs = [System.IO.File]::OpenRead($tiles)
    try { [void]$fs.Read($head, 0, 8) } finally { $fs.Dispose() }
    $magic = ($head | ForEach-Object { $_.ToString('x2') }) -join ''
    if ($magic -ne '89504e470d0a1a0a') {
        Stop-Fail "GET /api/tiles 返回的不是 PNG（前 8 字节 = $magic）"
    }
    if ($size -le 1000) {
        Write-Bad "GET /api/tiles 的 PNG 只有 $size 字节（≤1000，疑似空白区域）"
        Write-Say "      诊断：地图 (0,0) 起 ${TileW}×${TileH} 格可能是空的。"
        Write-Say '            换参数：$env:MIREDIT_TILE_W=64; $env:MIREDIT_TILE_H=48; .\verify.ps1'
        Stop-Fail 'tiles 太小'
    }
    Write-Ok "GET /api/tiles 200，PNG，$size 字节"

    # ---- ③ GET /api/stamps?q=market ------------------------------------
    $stamps = Join-Path $Tmp 'stamps.json'
    $code = Invoke-Curl -Url "$Base/api/stamps?q=market" -OutFile $stamps -MaxTime 60
    $body = Read-Text $stamps
    if ($code -ne '200') {
        Write-Bad "GET /api/stamps?q=market → HTTP $code"
        Write-Say ("      " + $body.Substring(0, [Math]::Min(400, $body.Length)))
        Write-Say '      诊断：503 通常是印章目录没挂上 —— MIREDIT_CATALOGUE_DIR 要指向'
        Write-Say '            CrystalStudio/catalogue（palette.json 就在里面）'
        Stop-Fail 'stamps 未通过'
    }
    $m = [regex]::Match($body, '"total"\s*:\s*(\d+)')
    if (-not $m.Success) { Stop-Fail "GET /api/stamps 的响应里找不到 total：$body" }
    $total = [int]$m.Groups[1].Value
    if ($total -le 0) { Stop-Fail 'GET /api/stamps?q=market 的 total = 0（印章目录里没有 market 条目）' }
    Write-Ok "GET /api/stamps?q=market 200，total = $total"

    # ---- ④ GET / （静态页）---------------------------------------------
    $index = Join-Path $Tmp 'index.html'
    $code = Invoke-Curl -Url "$Base/" -OutFile $index -MaxTime 15
    if ($code -ne '200') { Stop-Fail "GET / → HTTP $code（静态前端没进镜像？MIREDIT_WEB_ROOT 应为 /app/web）" }
    $hdrFile = Join-Path $Tmp 'hdr.txt'
    & curl.exe -sS --max-time 15 -o NUL -D $hdrFile "$Base/" 2>$null | Out-Null
    $hdr = Read-Text $hdrFile
    $cm = [regex]::Match($hdr, '(?im)^content-type:\s*([^\r\n]+)')
    $ctype = if ($cm.Success) { $cm.Groups[1].Value.Trim() } else { '<无>' }
    if ($ctype -notmatch '^text/html') { Stop-Fail "GET / 200 但 Content-Type=$ctype（期望 text/html）" }
    Write-Ok "GET / 200，Content-Type=$ctype，$((Get-Item -LiteralPath $index).Length) 字节"

    Write-Say '[5/5] 全部断言通过'

    # ----------------------------------------------------------- 6. 收工 ---
    if ($Keep) {
        Write-Say ''
        Write-Say "容器保留运行中（-Keep）。收工：docker compose -f $ComposeFile down"
    }
    else {
        Write-Say 'docker compose down ...'
        & docker compose -f $File down *> $null
    }

    Write-Say ''
    Write-Say 'DOCKER_VERIFY: PASS'
    exit 0
}
finally {
    Pop-Location
    Remove-Item -LiteralPath $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}
