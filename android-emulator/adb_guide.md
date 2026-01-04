# ADB 完整使用指导文档

<https://developer.android.com/tools/adb>

## 概述


Android Debug Bridge (ADB) 是一个多功能的命令行工具，用于与 Android 设备和模拟器进行通信。本文档提供了完整的 ADB 使用指导，涵盖从基础连接到高级功能的所有场景。

## 安装和配置

### 1. 安装 ADB

#### 方法一：通过 Android SDK
```bash
# 下载 Android SDK Platform Tools
# 设置环境变量
export ANDROID_HOME=/path/to/android/sdk
export PATH=$PATH:$ANDROID_HOME/platform-tools
```

#### 方法二：独立安装
```bash
# Ubuntu/Debian
sudo apt install android-tools-adb

# macOS (Homebrew)
brew install android-platform-tools

# Windows
# 下载 platform-tools 并添加到 PATH
```

### 2. 验证安装
```bash
# 检查 ADB 版本
adb version

# 检查 ADB 服务状态
adb devices
```

## 连接管理

### 1. 设备连接方式

#### USB 连接
```bash
# 列出所有连接的设备
adb devices

# 列出详细信息
adb devices -l
```

#### TCP/IP 连接
```bash
# 1. 首先通过 USB 连接设备
adb devices

# 2. 启用 TCP/IP 模式
adb tcpip 5555

# 3. 获取设备 IP 地址
adb shell ip addr show wlan0

# 4. 通过 TCP/IP 连接
adb connect 192.168.1.100:5555

# 5. 验证连接
adb devices
```

#### 无线调试 (Android 11+)
```bash
# 启用无线调试后
adb pair 192.168.1.100:37829
adb connect 192.168.1.100:37829
```

### 2. 多设备管理

```bash
# 指定设备执行命令
adb -s <device_id> <command>

# 示例
adb -s emulator-5554 shell
adb -s 192.168.1.100:5555 install app.apk

# 获取设备序列号
adb get-serialno

# 等待设备连接
adb wait-for-device
```

### 3. 连接状态管理

```bash
# 断开 TCP/IP 连接
adb disconnect 192.168.1.100:5555

# 断开所有连接
adb disconnect

# 重启 ADB 服务
adb kill-server
adb start-server

# 检查设备状态
adb get-state
```

## 基础操作

### 1. Shell 访问

```bash
# 进入设备 shell
adb shell

# 执行单个命令
adb shell ls /sdcard

# 以 root 权限执行 (需要 root)
adb root
adb shell

# 切换回普通用户
adb unroot
```

### 2. 设备重启

```bash
# 重启设备
adb reboot

# 重启到 bootloader
adb reboot bootloader

# 重启到 recovery
adb reboot recovery

# 重启到 fastboot
adb reboot fastboot
```

## 设备信息查询

### 1. 系统信息

```bash
# Android 版本
adb shell getprop ro.build.version.release

# API 级别
adb shell getprop ro.build.version.sdk

# 设备型号
adb shell getprop ro.product.model

# 设备制造商
adb shell getprop ro.product.manufacturer

# CPU 架构
adb shell getprop ro.product.cpu.abi

# 设备名称
adb shell getprop ro.product.name

# 构建版本
adb shell getprop ro.build.display.id

# 安全补丁级别
adb shell getprop ro.build.version.security_patch

# 内核版本
adb shell uname -a

# 系统启动时间
adb shell uptime
```

### 2. 硬件信息

```bash
# 内存信息
adb shell cat /proc/meminfo

# CPU 信息
adb shell cat /proc/cpuinfo

# 存储信息
adb shell df -h

# 电池信息
adb shell dumpsys battery

# 显示信息
adb shell dumpsys display

# 传感器信息
adb shell dumpsys sensorservice
```

### 3. 网络信息

```bash
# 网络接口
adb shell ip addr show

# 路由表
adb shell ip route

# WiFi 信息
adb shell dumpsys wifi

# 网络统计
adb shell cat /proc/net/dev

# DNS 配置
adb shell getprop net.dns1
adb shell getprop net.dns2
```

## 文件操作

### 1. 文件传输

```bash
# 从设备复制文件到本地
adb pull /sdcard/file.txt ./local_file.txt

# 从本地复制文件到设备
adb push local_file.txt /sdcard/file.txt

# 复制整个目录
adb pull /sdcard/Pictures ./pictures/
adb push ./data/ /sdcard/data/

# 同步目录 (保持时间戳)
adb sync
```

### 2. 文件管理

```bash
# 列出文件
adb shell ls -la /sdcard/

# 创建目录
adb shell mkdir -p /sdcard/test/

# 删除文件
adb shell rm /sdcard/file.txt

# 删除目录
adb shell rm -rf /sdcard/test/

# 移动/重命名文件
adb shell mv /sdcard/old.txt /sdcard/new.txt

# 复制文件
adb shell cp /sdcard/source.txt /sdcard/dest.txt

# 查看文件内容
adb shell cat /sdcard/file.txt

# 编辑文件 (简单)
adb shell "echo 'content' > /sdcard/file.txt"

# 文件权限
adb shell chmod 755 /sdcard/script.sh
```

### 3. 存储管理

```bash
# 查看存储使用情况
adb shell df -h

# 查看目录大小
adb shell du -sh /sdcard/

# 清理缓存
adb shell pm trim-caches 1000M

# 查看可用空间
adb shell stat -f /sdcard/
```

## 应用管理

### 1. 应用安装

```bash
# 安装 APK
adb install app.apk

# 强制安装 (覆盖)
adb install -r app.apk

# 安装到 SD 卡
adb install -s app.apk

# 允许降级安装
adb install -d app.apk

# 安装多个 APK (Split APK)
adb install-multiple base.apk config.apk

# 安装并授予所有权限
adb install -g app.apk
```

### 2. 应用卸载

```bash
# 卸载应用
adb uninstall com.example.app

# 卸载但保留数据
adb uninstall -k com.example.app

# 卸载系统应用 (需要 root)
adb shell pm uninstall --user 0 com.example.app
```

### 3. 应用信息查询

```bash
# 列出所有已安装的包
adb shell pm list packages

# 列出系统应用
adb shell pm list packages -s

# 列出第三方应用
adb shell pm list packages -3

# 列出已启用的应用
adb shell pm list packages -e

# 列出已禁用的应用
adb shell pm list packages -d

# 搜索特定应用
adb shell pm list packages | grep keyword

# 获取应用详细信息
adb shell dumpsys package com.example.app

# 获取应用路径
adb shell pm path com.example.app

# 获取应用版本
adb shell dumpsys package com.example.app | grep versionName
```

### 4. 应用控制

```bash
# 启动应用
adb shell am start -n com.example.app/.MainActivity

# 启动应用并传递数据
adb shell am start -a android.intent.action.VIEW -d "http://example.com"

# 停止应用
adb shell am force-stop com.example.app

# 清除应用数据
adb shell pm clear com.example.app

# 启用/禁用应用
adb shell pm enable com.example.app
adb shell pm disable com.example.app

# 授予权限
adb shell pm grant com.example.app android.permission.CAMERA

# 撤销权限
adb shell pm revoke com.example.app android.permission.CAMERA
```

## 日志管理

### 1. Logcat 基础

```bash
# 查看实时日志
adb logcat

# 查看最近的日志
adb logcat -t 100

# 清除日志缓冲区
adb logcat -c

# 查看缓冲区信息
adb logcat -g

# 查看统计信息
adb logcat -S
```

### 2. 日志过滤

```bash
# 按日志级别过滤
adb logcat *:E  # 只显示错误
adb logcat *:W  # 显示警告及以上
adb logcat *:I  # 显示信息及以上
adb logcat *:D  # 显示调试及以上
adb logcat *:V  # 显示所有级别

# 按标签过滤
adb logcat MyApp:D *:S

# 按进程 ID 过滤
adb logcat --pid=1234

# 按包名过滤
adb logcat --pid=$(adb shell pidof com.example.app)

# 使用正则表达式
adb logcat | grep "pattern"

# 多条件过滤
adb logcat MyApp:D System:W *:S
```

### 3. 日志格式

```bash
# 不同的输出格式
adb logcat -v brief     # 默认格式
adb logcat -v process   # 显示进程信息
adb logcat -v tag       # 只显示标签和消息
adb logcat -v raw       # 只显示消息
adb logcat -v time      # 显示时间戳
adb logcat -v threadtime # 显示线程时间
adb logcat -v long      # 显示详细信息

# 彩色输出
adb logcat -v color

# 输出到文件
adb logcat > logfile.txt
adb logcat -f /sdcard/logfile.txt
```

### 4. 特定缓冲区

```bash
# 查看不同缓冲区
adb logcat -b main      # 主缓冲区
adb logcat -b system    # 系统缓冲区
adb logcat -b radio     # 无线电缓冲区
adb logcat -b events    # 事件缓冲区
adb logcat -b crash     # 崩溃缓冲区

# 查看所有缓冲区
adb logcat -b all

# 同时查看多个缓冲区
adb logcat -b main -b system
```

## 网络和调试

### 1. 网络调试

```bash
# 端口转发
adb forward tcp:8080 tcp:8080

# 反向端口转发
adb reverse tcp:8080 tcp:8080

# 列出端口转发
adb forward --list

# 移除端口转发
adb forward --remove tcp:8080
adb forward --remove-all

# 网络连通性测试
adb shell ping google.com
adb shell nslookup google.com
adb shell netstat -an
```

### 2. 调试功能

```bash
# 启用/禁用调试模式
adb shell settings put global adb_enabled 1

# 屏幕截图
adb shell screencap /sdcard/screenshot.png
adb pull /sdcard/screenshot.png

# 屏幕录制
adb shell screenrecord /sdcard/recording.mp4
adb pull /sdcard/recording.mp4

# 输入事件
adb shell input text "Hello World"
adb shell input keyevent KEYCODE_HOME
adb shell input tap 500 500
adb shell input swipe 300 500 700 500

# 模拟按键
adb shell input keyevent 3    # HOME
adb shell input keyevent 4    # BACK
adb shell input keyevent 82   # MENU
adb shell input keyevent 26   # POWER
```

### 3. 系统设置

```bash
# 查看设置
adb shell settings list system
adb shell settings list secure
adb shell settings list global

# 修改设置
adb shell settings put system screen_brightness 100
adb shell settings put secure location_mode 3
adb shell settings put global wifi_on 1

# 获取特定设置
adb shell settings get system screen_brightness
```

## 性能监控

### 1. 系统性能

```bash
# CPU 使用率
adb shell top -n 1

# 内存使用情况
adb shell cat /proc/meminfo
adb shell dumpsys meminfo

# 进程信息
adb shell ps
adb shell ps -A

# 特定应用的内存使用
adb shell dumpsys meminfo com.example.app

# 系统负载
adb shell cat /proc/loadavg

# 磁盘 I/O
adb shell cat /proc/diskstats
```

### 2. 应用性能

```bash
# 应用启动时间
adb shell am start -W com.example.app/.MainActivity

# CPU 使用率 (特定应用)
adb shell top -p $(adb shell pidof com.example.app)

# GPU 使用情况
adb shell dumpsys gfxinfo com.example.app

# 网络使用情况
adb shell cat /proc/net/xt_qtaguid/stats

# 电池使用情况
adb shell dumpsys batterystats com.example.app
```

### 3. 系统监控

```bash
# 温度监控
adb shell cat /sys/class/thermal/thermal_zone*/temp

# 电池状态
adb shell dumpsys battery

# 传感器数据
adb shell dumpsys sensorservice

# 显示信息
adb shell dumpsys display

# 音频信息
adb shell dumpsys audio
```

## 高级功能

### 1. 系统服务

```bash
# 列出所有系统服务
adb shell service list

# 调用系统服务
adb shell service call phone 1

# 查看服务状态
adb shell dumpsys activity services

# 重启系统服务
adb shell stop service_name
adb shell start service_name
```

### 2. 数据库操作

```bash
# 访问应用数据库
adb shell run-as com.example.app
cd databases/
sqlite3 database.db

# 查看数据库内容
.tables
.schema table_name
SELECT * FROM table_name;

# 导出数据库
adb shell run-as com.example.app cat databases/db.sqlite > local_db.sqlite
```

### 3. 系统修改 (需要 root)

```bash
# 挂载系统分区为可写
adb shell mount -o remount,rw /system

# 修改系统文件
adb push modified_file /system/app/

# 修改权限
adb shell chmod 644 /system/app/file

# 重新挂载为只读
adb shell mount -o remount,ro /system
```

### 4. 备份和恢复

```bash
# 备份应用数据
adb backup -apk -shared -nosystem com.example.app

# 恢复应用数据
adb restore backup.ab

# 完整系统备份
adb backup -all -apk -shared -nosystem

# 备份到指定文件
adb backup -f backup.ab com.example.app
```

## 故障排除

### 1. 连接问题

```bash
# ADB 服务器问题
adb kill-server
adb start-server

# 设备未识别
adb devices
# 检查 USB 调试是否启用
# 检查 USB 驱动是否正确安装

# 权限问题 (Linux/macOS)
sudo adb devices

# 端口占用问题
netstat -an | grep 5037
lsof -i :5037
```

### 2. 设备状态问题

```bash
# 设备离线
adb devices
adb reconnect

# 设备未授权
# 在设备上确认 USB 调试授权

# 设备无响应
adb shell
# 如果无响应，尝试重启设备
adb reboot
```

### 3. 常见错误

```bash
# "device not found" 错误
adb devices
adb kill-server && adb start-server

# "insufficient permissions" 错误
# 检查 udev 规则 (Linux)
# 检查设备驱动 (Windows)

# "protocol fault" 错误
adb disconnect
adb connect device_ip:port

# 超时错误
# 增加超时时间或检查网络连接
```

## 最佳实践

### 1. 安全考虑

```bash
# 生产环境中禁用 ADB
adb shell settings put global adb_enabled 0

# 使用 TCP/IP 时注意网络安全
# 只在可信网络中使用
# 及时断开不需要的连接

# 避免在公共网络中使用无线调试
```

### 2. 性能优化

```bash
# 使用特定设备 ID 避免歧义
adb -s device_id command

# 批量操作时使用脚本
#!/bin/bash
for device in $(adb devices | grep device | cut -f1); do
    adb -s $device shell command
done

# 长时间监控时使用文件输出
adb logcat > logfile.txt &
```

### 3. 自动化脚本

```bash
#!/bin/bash
# ADB 自动化脚本示例

# 等待设备连接
adb wait-for-device

# 安装应用
adb install -r app.apk

# 启动应用
adb shell am start -n com.example.app/.MainActivity

# 等待应用启动
sleep 5

# 执行测试
adb shell input tap 500 500

# 获取日志
adb logcat -t 100 > test_log.txt

# 截图
adb shell screencap /sdcard/test.png
adb pull /sdcard/test.png
```

### 4. 调试技巧

```bash
# 实时监控特定应用日志
adb logcat | grep "$(adb shell ps | grep com.example.app | awk '{print $2}')"

# 监控网络请求
adb shell tcpdump -i any -w /sdcard/capture.pcap

# 性能分析
adb shell am profile start com.example.app /sdcard/profile.trace
# 执行操作
adb shell am profile stop com.example.app

# 内存泄漏检测
adb shell dumpsys meminfo com.example.app
```

## 常用命令速查表

### 设备和连接
```bash
adb devices                    # 列出所有设备
adb devices -l                # 列出设备详细信息
adb connect ip:port           # TCP/IP 连接
adb disconnect               # 断开连接
adb disconnect ip:port       # 断开指定连接
adb tcpip 5555              # 启用 TCP/IP 模式
adb usb                     # 启用 USB 模式
adb -s device_id command    # 在指定设备执行命令
```

### 应用管理
```bash
adb install app.apk                   # 安装应用
adb install -r app.apk               # 重新安装应用
adb install -g app.apk               # 自动授予权限
adb uninstall package.name            # 卸载应用
adb shell pm list packages            # 列出所有包
adb shell am start activity           # 启动应用
adb shell am start -n package/.Activity  # 启动指定 Activity
adb shell am force-stop package       # 强制停止应用
adb shell getprop ro.build.version.release  # 查看 Android 版本
```

### 文件操作
```bash
adb push local_file /sdcard/         # 上传文件到设备
adb pull /sdcard/file local_file     # 从设备下载文件
adb shell ls path                    # 列出文件
adb shell rm file                    # 删除文件
adb shell mkdir directory            # 创建目录
adb shell cd path                    # 进入目录
adb shell cat file                   # 查看文件内容
```

### 日志和调试
```bash
adb logcat                           # 查看日志
adb logcat -c                        # 清除日志
adb logcat -n 5                      # 查看最后 5 行
adb logcat | grep TAG               # 过滤日志
adb logcat -s TAG                   # 只显示 TAG 日志
adb logcat *:E                      # 只显示错误日志
adb shell screencap -p /sdcard/ss.png    # 截图
adb shell screenrecord /sdcard/video.mp4 # 录屏
adb shell input text "hello"        # 输入文本
adb shell input tap x y             # 点击屏幕
adb shell input swipe x1 y1 x2 y2   # 滑动操作
```

### 模拟器控制
```bash
adb shell                            # 进入 shell
adb shell exit                       # 退出 shell
adb emu kill                         # 关闭模拟器
adb shell reboot                     # 重启设备
adb shell settings put global airplane_mode_on 1  # 启用飞行模式
adb shell settings put global airplane_mode_on 0  # 禁用飞行模式
```

### 系统信息
```bash
adb shell getprop                    # 查看所有系统属性
adb shell getprop ro.product.model   # 查看设备型号
adb shell getprop ro.build.version.sdk  # 查看 SDK 版本
adb shell dumpsys service            # 系统服务信息
adb shell ps                         # 进程列表
adb shell top                        # 系统资源使用
adb shell cat /proc/meminfo          # 内存信息
adb shell df                         # 磁盘空间
```

### 本地和网络连接示例
```bash
# 本地连接（USB 或本地仿真器）
adb devices
# 输出示例：emulator-5554   device

# 使用本地连接查看日志
adb -s emulator-5554 logcat

# TCP/IP 网络连接
adb connect 192.168.1.100:5555

# 使用网络连接查看日志
adb -s 192.168.1.100:5555 logcat

# 查看连接中的所有设备
adb devices -l
# 输出示例：
# emulator-5554          device usb:1-1 product:emulator-x86 model:Android_SDK_built_for_x86 device:generic_x86
# 192.168.1.100:5555     device
```

## 结语

本文档涵盖了 ADB 的所有主要功能和使用场景。ADB 是 Android 开发和调试的强大工具，掌握这些命令将大大提高你的开发效率。

建议根据实际需求选择合适的命令，并在使用前充分测试，特别是涉及系统修改的操作。

对于远程 ADB 日志系统的开发，重点关注连接管理、日志处理和网络调试相关的命令。