# OpenWRT Init Script

```bash
#!/bin/sh /etc/rc.common

USE_PROCD=1
START=95        # start after network (90) and HA adapter (94)
STOP=10

start_service() {
    procd_open_instance
    procd_set_param command /usr/bin/smarthome-daemon
    procd_set_param env HA_TOKEN="$(cat /etc/smarthome/ha_token)"
    procd_set_param respawn 60 5 0   # restart after 60s if crashes
    procd_set_param limits nofile="4096"
    procd_set_param stdout 1
    procd_set_param stderr 1
    procd_close_instance
}

START=95 ensures network and HA are ready
procd auto-restarts if daemon crashes
```