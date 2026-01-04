# Android 模拟器快速指南

直接使用核心工具 (`avdmanager`, `sdkmanager`, `emulator`) 管理 Android 虚拟设备。

## 完整流程图

```mermaid
flowchart LR
    A[开始] --> B[检查环境变量]
    B --> C[检查系统架构]
    C --> D{架构类型}
    D -->|arm64| E[使用 ARM64 镜像]
    D -->|x86_64| F[使用 x86_64 镜像]
    E --> G[查找可用系统镜像]
    F --> G
    G --> H{镜像存在?}
    H -->|否| I[安装系统镜像]
    H -->|是| J[创建 AVD]
    I --> J
    J --> K{创建成功?}
    K -->|否| L[检查镜像路径格式]
    K -->|是| M[启动模拟器]
    L --> J
    M --> N[等待启动完成]
    N --> O[验证连接]
    O --> P[使用模拟器]
    P --> Q[关闭模拟器]
    Q --> R[删除 AVD]
    R --> S[结束]
```

## 前置条件

### Linux/macOS

确保已设置环境变量：
```bash
export ANDROID_HOME=$HOME/Android/Sdk
export PATH=$PATH:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin
```

### Windows

**本机配置示例（SDK 路径：D:\zoo\Android\Sdk）**

在 PowerShell 中设置环境变量（临时）：
```powershell
$env:ANDROID_HOME = "D:\zoo\Android\Sdk"
$env:PATH = "$env:PATH;$env:ANDROID_HOME\emulator;$env:ANDROID_HOME\platform-tools;$env:ANDROID_HOME\cmdline-tools\latest\bin"
```

或在 CMD 中（临时）：
```cmd
set ANDROID_HOME=D:\zoo\Android\Sdk
set PATH=%PATH%;%ANDROID_HOME%\emulator;%ANDROID_HOME%\platform-tools;%ANDROID_HOME%\cmdline-tools\latest\bin
```

**永久设置环境变量（推荐）：**

PowerShell（管理员权限）：
```powershell
[System.Environment]::SetEnvironmentVariable('ANDROID_HOME', 'D:\zoo\Android\Sdk', 'User')
$currentPath = [System.Environment]::GetEnvironmentVariable('Path', 'User')
$newPath = "$currentPath;D:\zoo\Android\Sdk\emulator;D:\zoo\Android\Sdk\platform-tools;D:\zoo\Android\Sdk\cmdline-tools\latest\bin"
[System.Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
```

或通过系统设置：
1. 右键"此电脑" → "属性" → "高级系统设置" → "环境变量"
2. 在"用户变量"中新建：
   - 变量名：`ANDROID_HOME`
   - 变量值：`D:\zoo\Android\Sdk`
3. 编辑 `Path` 变量，添加：
   - `D:\zoo\Android\Sdk\emulator`
   - `D:\zoo\Android\Sdk\platform-tools`
   - `D:\zoo\Android\Sdk\cmdline-tools\latest\bin`

**通用配置（适用于标准安装路径）：**

PowerShell:
```powershell
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:PATH = "$env:PATH;$env:ANDROID_HOME\emulator;$env:ANDROID_HOME\platform-tools;$env:ANDROID_HOME\cmdline-tools\latest\bin"
```

CMD:
```cmd
set ANDROID_HOME=%LOCALAPPDATA%\Android\Sdk
set PATH=%PATH%;%ANDROID_HOME%\emulator;%ANDROID_HOME%\platform-tools;%ANDROID_HOME%\cmdline-tools\latest\bin
```

### 架构兼容性检查

**重要：** 在 Apple Silicon (ARM64) Mac 上，必须使用 ARM64 系统镜像，不能使用 x86_64 镜像。

检查系统架构：
```bash
uname -m
# arm64 = Apple Silicon Mac，需要使用 arm64-v8a 镜像
# x86_64 = Intel Mac，可以使用 x86_64 镜像
```

## 快速开始

### 完整流程示例（Apple Silicon Mac）

```bash
# 1. 检查系统架构
uname -m  # 应该输出 arm64

# 2. 检查可用的 ARM64 镜像
find $ANDROID_HOME/system-images -maxdepth 3 -type d -name "arm64-v8a" 2>/dev/null | sed 's|.*/system-images/||'

# 3. 创建 AVD
avdmanager create avd \
  -n test_emulator \
  -d pixel_6 \
  -k "system-images;android-34;google_apis_playstore;arm64-v8a" \
  -c 2048M \
  -f

# 4. 启动模拟器（后台运行）
emulator -avd test_emulator -no-window -verbose &

# 5. 等待启动完成（约30-60秒）
while [ "$(adb shell getprop sys.boot_completed 2>/dev/null)" != "1" ]; do
  echo "等待启动..."
  sleep 3
done
echo "✓ 模拟器已就绪"

# 6. 验证连接
adb devices

# 7. 清理（可选）
adb emu kill
avdmanager delete avd -n test_emulator
```

## 核心操作

### 1. 列出可用 AVD

查看已创建的所有虚拟设备：

```bash
emulator -list-avds
```

### 2. 创建 AVD

#### 查看可用系统镜像

**查看本地已安装的镜像（推荐）：**

对于 Apple Silicon (ARM64) Mac：
```bash
find $ANDROID_HOME/system-images -maxdepth 3 -type d -name "arm64-v8a" 2>/dev/null | sed 's|.*/system-images/||'
```

对于 Intel (x86_64) Mac：
```bash
find $ANDROID_HOME/system-images -maxdepth 3 -type d -name "x86_64" 2>/dev/null | sed 's|.*/system-images/||'
```

示例输出（ARM64 Mac）：
```
android-34/google_apis_playstore/arm64-v8a
```

示例输出（Intel Mac）：
```
android-34/google_apis_playstore/x86_64
```

**如果没有找到镜像，需要先安装：**
```bash
# ARM64 Mac
sdkmanager "system-images;android-34;google_apis_playstore;arm64-v8a"

# Intel Mac
sdkmanager "system-images;android-34;google_apis_playstore;x86_64"
```

**查看 SDK 中所有可用镜像：**
```bash
sdkmanager --list | grep "system-images"
```

**查看本地已安装的镜像（推荐）：**

Linux/macOS:
#### 创建新 AVD

**Apple Silicon (ARM64) Mac：**
```bash
find $ANDROID_HOME/system-images -maxdepth 3 -type d -name "x86_64" 2>/dev/null | sed 's|.*/system-images/||'
```

Windows (PowerShell):
```powershell
Get-ChildItem -Path "$env:ANDROID_HOME\system-images" -Recurse -Directory -Filter "x86_64" | ForEach-Object { $_.FullName -replace '.*\\system-images\\', '' }
```

Windows (CMD):
```cmd
dir /s /b "%ANDROID_HOME%\system-images\*x86_64" | findstr /R "system-images"
```

**本机示例（D:\zoo\Android\Sdk）：**
```powershell
Get-ChildItem -Path "D:\zoo\Android\Sdk\system-images" -Recurse -Directory -Filter "x86_64" | ForEach-Object { $_.FullName -replace '.*\\system-images\\', '' }
```

本机输出示例：
```
android-35\google_apis\x86_64
```

通用输出示例：
```
android-36/google_apis_playstore/x86_64
android-36.1/google_apis_playstore/x86_64
avdmanager create avd \
  -n my_emulator \
  -d pixel_6 \
  -k "system-images;android-34;google_apis_playstore;arm64-v8a" \
  -c 2048M \
  -f
```

**只有这样列出的镜像才是真实存在于本地的，可用于创建 AVD。**

使用 `avdmanager` 创建虚拟设备：

**本机示例（使用 android-35）：**
```bash
avdmanager create avd \
  -n endorphin_test \
  -d pixel_6 \
  -k "system-images;android-35;google_apis;x86_64" \
  -f
```

**通用示例：**
**Intel (x86_64) Mac：**
```bash
avdmanager create avd \
  -n my_emulator \
  -d pixel_6 \
  -k "system-images;android-34;google_apis_playstore;x86_64" \
  -c 2048M \
  -f
```

**如果出现 "Package path is not valid" 错误：**
1. 确保系统镜像已正确安装
2. 验证镜像目录存在：
   ```bash
   # ARM64 Mac
   ls -la $ANDROID_HOME/system-images/android-34/google_apis_playstore/arm64-v8a/
   
   # Intel Mac
   ls -la $ANDROID_HOME/system-images/android-34/google_apis_playstore/x86_64/
   ```
3. 重新安装系统镜像（见上面的安装命令）

参数说明：
- `-n my_emulator` - AVD 名称
- `-d pixel_6` - 设备配置（可用：pixel_6, pixel_5, pixel_4, Nexus_5X 等）
- `-k "system-images;..."` - 系统镜像路径
- `-c 2048M` - SD 卡大小
- `-f` - 若已存在则覆盖

**查看所有可用设备定义：**
```bash
avdmanager list device
```

**创建后优化配置（可选）：**

编辑 AVD 配置文件以提升性能：
- Windows: `%USERPROFILE%\.android\avd\<avd_name>.avd\config.ini`
- Linux/macOS: `~/.android/avd/<avd_name>.avd/config.ini`

推荐配置：
```ini
hw.ramSize = 6144          # 内存 6GB（根据系统内存调整，建议 4-8GB）
hw.cpu.ncore = 4           # CPU 核心数（根据系统 CPU 调整）
hw.gpu.enabled = yes       # 启用 GPU
hw.gpu.mode = host         # 使用主机 GPU（性能最佳）
```

**本机优化脚本（PowerShell）：**
```powershell
# 修改 AVD 配置
$avdPath = "$env:USERPROFILE\.android\avd\endorphin_test.avd\config.ini"
$config = Get-Content $avdPath
$config = $config -replace 'hw.ramSize\s*=\s*.*', 'hw.ramSize = 6144'
$config = $config -replace 'hw.gpu.enabled\s*=\s*.*', 'hw.gpu.enabled = yes'
$config = $config -replace 'hw.gpu.mode\s*=\s*.*', 'hw.gpu.mode = host'
$config | Set-Content $avdPath


### 3. 启动模拟器

#### 检查硬件加速支持

启动前先检查系统支持的加速方式：
```bash
emulator -accel-check
```

#### 基础启动（推荐）

**本机启动（Windows，已优化 GPU）：**
```powershell
emulator -avd endorphin_test
```

**通用启动：**
```bash
emulator -avd my_emulator
```

#### 推荐启动方式（后台运行）
模拟器会自动使用 config.ini 中配置的 GPU 和内存设置。

#### 带选项启动（高级）

**Windows (本机推荐配置):**

如果 AVD 已配置 GPU，直接启动即可：
```powershell
emulator -avd endorphin_test
```

如果需要覆盖 GPU 设置：
```powershell
# 使用主机 GPU（性能最佳，需要良好的驱动支持）
emulator -avd endorphin_test -gpu host

**macOS 推荐启动命令：**
# 使用软件渲染（兼容性最好，性能较低）
emulator -avd endorphin_test -gpu swiftshader_indirect

# 使用 ANGLE（Windows 推荐，平衡性能和兼容性）
emulator -avd endorphin_test -gpu angle_indirect
```

**Linux:**
```bash
emulator -avd my_emulator \
  -verbose \
  -show-kernel \
  -gpu swangle_indirect \
  -accel hvf \
  -no-window
  -gpu host \
  -accel kvm
```

**macOS:**
```bash
emulator -avd my_emulator \
  -gpu host \
  -accel hvf
```

**如果遇到架构不兼容错误（Apple Silicon Mac）：**
```
PANIC: Avd's CPU Architecture 'x86_64' is not supported by the QEMU2 emulator on aarch64 host
```

解决方案：
1. 检查系统架构：`uname -m`
2. 如果输出是 `arm64`，必须使用 ARM64 系统镜像
3. 删除现有的 x86_64 AVD：`avdmanager delete avd -n <avd_name>`
4. 创建新的 ARM64 AVD（见上面第2节）

#### 启动选项说明

常用选项：
- `-verbose` - 显示详细日志
- `-show-kernel` - 显示内核启动信息
- `-gpu swangle_indirect` - 使用软件渲染（兼容性更好）
- `-accel hvf` - 启用 HVF 硬件加速（macOS，推荐）
#### GPU 模式选择指南

- `auto` (默认) - 自动选择，推荐用于大多数情况
- `host` - 使用主机 GPU，性能最佳，需要良好的驱动支持
- `angle_indirect` - Windows 推荐，使用 DirectX，平衡性能和兼容性
- `swiftshader_indirect` - 软件渲染，兼容性最好但性能较低
- `guest` - 客户端渲染，非常慢，不推荐

#### 常用启动选项

- `-verbose` - 显示详细日志（调试用）
- `-show-kernel` - 显示内核启动信息（调试用）
- `-gpu <mode>` - 指定 GPU 模式
- `-accel kvm` - 启用 KVM 硬件加速（Linux）
- `-accel whpx` - 启用 WHPX 硬件加速（Windows Hyper-V）
- `-accel hvf` - 启用 HVF 硬件加速（macOS）
- `-audio pulse` - 使用 PulseAudio（Linux）
- `-no-window` - 无窗口模式（后台运行）
- `-port 5554` - 指定 ADB 端口（默认 5554）
- `-memory <size>` - 覆盖内存大小（如 `-memory 6144`）
 

**注意：** 模拟器首次启动可能需要 1-3 分钟，请耐心等待。后续启动会更快。

**如果启动缓慢或卡在启动画面：**
1. 确保使用正确的硬件加速：
   ```bash
   # macOS - 检查 HVF 支持
   sysctl kern.hv_support
   # 输出应该是 kern.hv_support: 1
   ```

2. 如果 HVF 不可用，使用软件加速：
   ```bash
   emulator -avd my_emulator -accel off
   ```

3. 增加内存分配：
   ```bash
   emulator -avd my_emulator -memory 4096
   ```

4. 使用冷启动：
   ```bash
   emulator -avd my_emulator -no-snapshot-load
   ```

**如果 GPU 加速导致崩溃：**
```bash
emulator -avd my_emulator -gpu swangle_indirect
```

### 4. 检查模拟器状态

### 5. 删除 AVD

**使用 avdmanager（推荐）：**
```bash
avdmanager delete avd -n my_emulator
```

#### 方法一：使用 avdmanager（推荐）

Linux/macOS:
```bash
# 列出所有 AVD
emulator -list-avds

# 删除特定 AVD
avdmanager delete avd -n my_emulator
```

#### 方法二：手动删除文件

```bash
# 删除 AVD 目录和配置文件
rm -rf ~/.android/avd/my_emulator.avd
rm ~/.android/avd/my_emulator.ini
```

Windows (PowerShell):
```powershell
# 列出所有 AVD
emulator -list-avds

# 删除特定 AVD
Remove-Item -Recurse -Force "$env:USERPROFILE\.android\avd\my_emulator.avd"
Remove-Item -Force "$env:USERPROFILE\.android\avd\my_emulator.ini"
```

### 问题：硬件加速不可用

**Linux (KVM):**
```bash
# 启用 KVM
sudo modprobe kvm-intel  # Intel CPU
sudo modprobe kvm-amd    # AMD CPU

# 将用户加入 kvm 组
sudo usermod -a -G kvm $USER
# 注销并重新登录生效
```

**Windows (WHPX):**
1. 确保启用了 Hyper-V 和 Windows Hypervisor Platform
2. 在"启用或关闭 Windows 功能"中勾选：
   - Hyper-V
   - Windows Hypervisor Platform
3. 重启计算机

**macOS (HVF):**
- macOS 10.10+ 自动支持 HVF，无需额外配置

### 问题：GPU 加速导致崩溃

尝试软件渲染：
```bash
emulator -avd my_emulator -gpu swiftshader_indirect
#### 强制关闭模拟器

如果模拟器正在运行，先关闭：
```bash
# 优雅关闭
adb emu kill

# 或强制杀死进程
pkill -f emulator
```

## 相关文档

- [adb_guide.md](adb_guide.md) - 详细的 ADB 命令参考
