# Android 模拟器快速指南

直接使用核心工具 (`avdmanager`, `sdkmanager`, `emulator`, `adb`) 管理 Android 虚拟设备。

## 前置条件

确保已设置环境变量：
```bash
export ANDROID_HOME=$HOME/Android/Sdk
export PATH=$PATH:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin
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
```bash
find $ANDROID_HOME/system-images -maxdepth 3 -type d -name "x86_64" 2>/dev/null | sed 's|.*/system-images/||'
```

示例输出（已本地存在）：
```
android-36/google_apis_playstore/x86_64
android-36.1/google_apis_playstore/x86_64
```

**只有这样列出的镜像才是真实存在于本地的，可用于创建 AVD。**

#### 创建新 AVD

使用 `avdmanager` 创建虚拟设备：

```bash
avdmanager create avd \
  -n my_emulator \
  -d pixel_6 \
  -k "system-images;android-36;google_apis_playstore;x86_64" \
  -c 4096M \
  -f
```

参数说明：
- `-n my_emulator` - AVD 名称
- `-d pixel_6` - 设备配置（可用：pixel_6, pixel_5, pixel_4, Nexus_5X 等）
- `-k "system-images;..."` - 系统镜像路径
- `-c 4096M` - SD 卡大小
- `-f` - 若已存在则覆盖

#### 手动创建 AVD（当 avdmanager 失败时）

如果 avdmanager 出现 "Package path is not valid" 错误，可手动创建配置：

```bash
# 创建 AVD 目录
mkdir -p ~/.android/avd/my_emulator.avd

# 创建 config.ini
cat > ~/.android/avd/my_emulator.avd/config.ini << 'EOF'
hw.accelerometer=yes
hw.audioInput=yes
hw.audioOutput=yes
hw.battery=yes
hw.camera.back=emulated
hw.camera.front=emulated
hw.cpu.arch=x86_64
hw.cpu.ncore=4
hw.dpad=no
hw.gps=yes
hw.gpu.enabled=yes
hw.gpu.mode=swiftshader_indirect
hw.gyroscope=yes
hw.initialOrientation=Portrait
hw.keyboard=yes
hw.lcd.density=420
hw.lcd.height=2400
hw.lcd.width=1080
hw.mainKeys=no
hw.ramSize=4096
hw.sdCard=yes
hw.sdCard.size=512M
hw.sensors.orientation=yes
hw.sensors.proximity=yes
image.sysdir.1=system-images/android-36/google_apis_playstore/x86_64/
kernel.qemu=yes
vm.heapSize=512
EOF

# 创建 ini 条目文件
cat > ~/.android/avd/my_emulator.ini << 'EOF'
avd.ini.encoding=UTF-8
path=/home/hanl5/.android/avd/my_emulator.avd
path.rel=avd/my_emulator.avd
target=android-36
EOF
```

验证创建成功：
```bash
emulator -list-avds | grep my_emulator
```

### 3. 启动模拟器

#### 基础启动

```bash
emulator -avd my_emulator
```

#### 带选项启动（推荐）

```bash
emulator -avd my_emulator \
  -verbose \
  -show-kernel \
  -gpu swiftshader_indirect \
  -accel kvm \
  -audio pulse \
  -no-window
```

常用选项：
- `-verbose` - 显示详细日志
- `-show-kernel` - 显示内核启动信息
- `-gpu swiftshader_indirect` - 使用软件渲染（兼容性更好）
- `-accel kvm` - 启用 KVM 硬件加速（Linux）
- `-accel whpx` - 启用 WHPX 硬件加速（Windows Hyper-V）
- `-accel hvf` - 启用 HVF 硬件加速（macOS）
- `-audio pulse` - 使用 PulseAudio（Linux）
- `-no-window` - 无窗口模式（后台运行）
- `-port 5554` - 指定 ADB 端口（默认 5554）

### 4. 检查模拟器状态

#### 等待模拟器就绪

```bash
# 轮询检查设备状态，直到显示 "device"
while [ "$(adb shell getprop sys.boot_completed 2>/dev/null)" != "1" ]; do
  echo "等待启动..."
  sleep 2
done
echo "✓ 模拟器已就绪"
```

或简单方法：
```bash
# 检查 adb 设备列表
adb devices

# 预期输出：
# emulator-5554   device
```

#### 连接到特定模拟器

```bash
# 如果运行多个模拟器，指定端口
adb -s emulator-5554 shell
```

### 5. 删除 AVD

```bash
# 列出所有 AVD
emulator -list-avds

# 删除特定 AVD
rm -rf ~/.android/avd/my_emulator.avd
rm ~/.android/avd/my_emulator.ini
```

### 6. 常用 ADB 命令

```bash
# 列出所有设备
adb devices

# 连接到模拟器 shell
adb shell

# 安装应用
adb install /path/to/app.apk

# 查看日志
adb logcat

# 取出文件
adb pull /sdcard/file.txt ./

# 推送文件
adb push ./file.txt /sdcard/

# 重启模拟器
adb reboot

# 关闭 adb 服务
adb kill-server

# 启动 adb 服务
adb start-server
```

## 故障排除

### 问题：avdmanager create 显示 "Package path is not valid"

**原因：** SDK 包清单未识别系统镜像，或 sdkmanager 未正确注册包。

**解决方案：**
1. 使用手动创建 AVD 的方法（见上面第 2 节）
2. 验证系统镜像目录存在：
   ```bash
   ls -la $ANDROID_HOME/system-images/android-36/google_apis_playstore/x86_64/
   ```

### 问题：emulator 启动失败或卡住

**解决方案：**
```bash
# 用详细日志启动
emulator -avd my_emulator -verbose -show-kernel 2>&1 | tee emulator.log

# 检查日志找到具体错误
```

### 问题：KVM 不可用（Linux）

```bash
# 启用 KVM
sudo modprobe kvm-intel  # Intel CPU
sudo modprobe kvm-amd    # AMD CPU

# 将用户加入 kvm 组
sudo usermod -a -G kvm $USER
# 注销并重新登录生效
```

### 问题：GPU 加速导致崩溃

尝试软件渲染：
```bash
emulator -avd my_emulator -gpu swiftshader_indirect
```

## 相关文档

- [adb_guide.md](adb_guide.md) - 详细的 ADB 命令参考
