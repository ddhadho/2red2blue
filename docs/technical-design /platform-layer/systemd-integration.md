# Linux / Pi systemd Unit

[Unit]
Description=Smarthome Daemon
After=network.target home-assistant.service
Wants=home-assistant.service

[Service]
Type=simple
User=smarthome
ExecStart=/usr/local/bin/smarthome-daemon
EnvironmentFile=/etc/smarthome/environment
Restart=always
RestartSec=10
MemoryMax=256M
CPUQuota=50%

[Install]
WantedBy=multi-user.target

Restart=always recovers from crashes
Memory/CPU caps enforce resource limits