# Android 模拟器快速指南

直接使用核心工具 (`avdmanager`, `sdkmanager`, `emulator`, `adb`) 管理 Android 虚拟设备。

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

## 核心操作

### 1. 列出可用 AVD

查看已创建的所有虚拟设备：

```bash
emulator -list-avds
```

或使用 adb 查看正在运行的设备：

```bash
adb devices
```

### 2. 创建 AVD

#### 查看可用系统镜像

**查看 SDK 中所有可用镜像：**
```bash
sdkmanager --list | grep "system-images"
```

**查看本地已安装的镜像（推荐）：**

Linux/macOS:
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
```

**只有这样列出的镜像才是真实存在于本地的，可用于创建 AVD。**

#### 创建新 AVD

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
```bash
avdmanager create avd \
  -n my_emulator \
  -d pixel_6 \
  -k "system-images;android-36;google_apis_playstore;x86_64" \
  -f
```

参数说明：
- `-n my_emulator` - AVD 名称
- `-d pixel_6` - 设备配置（可用：pixel_6, pixel_5, pixel_4, Nexus_5X 等）
- `-k "system-images;..."` - 系统镜像路径（必须与本地已安装的镜像匹配）
- `-f` - 若已存在则覆盖

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
```

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

# 使用软件渲染（兼容性最好，性能较低）
emulator -avd endorphin_test -gpu swiftshader_indirect

# 使用 ANGLE（Windows 推荐，平衡性能和兼容性）
emulator -avd endorphin_test -gpu angle_indirect
```

**Linux:**
```bash
emulator -avd my_emulator \
  -gpu host \
  -accel kvm
```

**macOS:**
```bash
emulator -avd my_emulator \
  -gpu host \
  -accel hvf
```

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
- `-no-window` - 无窗口模式（后台运行）
- `-port 5554` - 指定 ADB 端口（默认 5554）
- `-memory <size>` - 覆盖内存大小（如 `-memory 6144`）
 

### 5. 删除 AVD

**使用 avdmanager（推荐）：**
```bash
avdmanager delete avd -n my_emulator
```

**手动删除：**

Linux/macOS:
```bash
# 列出所有 AVD
emulator -list-avds

# 删除特定 AVD
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
```

## 相关文档

- [adb_guide.md](adb_guide.md) - 详细的 ADB 命令参考
