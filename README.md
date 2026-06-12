# tower-api

REST API for the Adafruit USB Tri-Color Tower Light with Buzzer.  
Runs on the Jetson (or any Linux box). Starts with **yellow on** to signal boot.

---

## Quick start

```bash
cargo build --release
sudo cp target/release/tower-api /usr/local/bin/
sudo cp 99-tower-light.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo cp tower-api.service /etc/systemd/system/
sudo systemctl enable --now tower-api
```

The API listens on **port 3000** by default.  
Override with `TOWER_BIND=0.0.0.0:8080`.

---

## Port resolution order

1. CLI argument: `tower-api /dev/ttyUSB1`
2. Autodetect by USB VID/PID (CH340: `1a86:7523`)
3. Fallback to `/dev/tower-light` (udev symlink)

---

## Channels

| Name     | Description        |
|----------|--------------------|
| `red`    | Red LED segment    |
| `yellow` | Yellow LED segment |
| `green`  | Green LED segment  |
| `buzzer` | Audible buzzer     |

---

## Endpoints

### GET /status
Returns the full state machine snapshot.

```json
{
  "red":    "off",
  "yellow": { "sw_blink": { "on_ms": 300, "off_ms": 300 } },
  "green":  "on",
  "buzzer": "off",
  "last_updated": "2024-01-15T10:30:00Z"
}
```

---

### POST /clear
Turn everything off and cancel all blink tasks.

```bash
curl -X POST http://jetson:3000/clear
```

---

### POST /set
Set multiple channels atomically. Omit a channel to leave it unchanged.

```bash
curl -X POST http://jetson:3000/set \
  -H 'Content-Type: application/json' \
  -d '{"red": true, "green": false, "buzzer": false}'
```

---

### POST /:channel/on
Turn a channel on immediately.

```bash
curl -X POST http://jetson:3000/green/on
curl -X POST http://jetson:3000/buzzer/on
```

---

### POST /:channel/off  /  DELETE /:channel
Turn a channel off immediately. Cancels any running blink task.

```bash
curl -X POST http://jetson:3000/red/off
curl -X DELETE http://jetson:3000/red
```

---

### POST /:channel/blink/hw
Hardware native blink (~1 Hz, fixed frequency). Least CPU overhead.

```bash
curl -X POST http://jetson:3000/yellow/blink/hw
```

---

### POST /:channel/blink
Software blink with custom frequency. Runs indefinitely until cancelled.

**Body** (all fields optional):
| Field    | Default | Description              |
|----------|---------|--------------------------|
| `on_ms`  | 500     | ON duration in ms        |
| `off_ms` | 500     | OFF duration in ms       |

```bash
# Fast urgent blink
curl -X POST http://jetson:3000/red/blink \
  -H 'Content-Type: application/json' \
  -d '{"on_ms": 100, "off_ms": 100}'

# Slow heartbeat
curl -X POST http://jetson:3000/green/blink \
  -H 'Content-Type: application/json' \
  -d '{"on_ms": 200, "off_ms": 1800}'
```

---

### POST /:channel/pulse
Blink exactly N times then turn off automatically.

**Body:**
| Field    | Default | Description              |
|----------|---------|--------------------------|
| `count`  | —       | Number of blinks (required) |
| `on_ms`  | 200     | ON duration in ms        |
| `off_ms` | 200     | OFF duration in ms       |

```bash
# Three quick red flashes (error notification)
curl -X POST http://jetson:3000/red/pulse \
  -H 'Content-Type: application/json' \
  -d '{"count": 3, "on_ms": 150, "off_ms": 150}'

# Single long buzzer beep
curl -X POST http://jetson:3000/buzzer/pulse \
  -H 'Content-Type: application/json' \
  -d '{"count": 1, "on_ms": 500, "off_ms": 0}'
```

---

### POST /:channel/timed
Turn on for a fixed duration then off automatically.

**Body:**
| Field         | Description                  |
|---------------|------------------------------|
| `duration_ms` | How long to stay on (ms)     |

```bash
# Green on for 2 seconds
curl -X POST http://jetson:3000/green/timed \
  -H 'Content-Type: application/json' \
  -d '{"duration_ms": 2000}'
```

---

### POST /:channel/sequence
Execute a custom step pattern once, then off.  
Useful for morse code, attention patterns, boot animations.

**Body:**
```json
{
  "steps": [
    { "on_ms": 200, "off_ms": 100 },
    { "on_ms": 200, "off_ms": 100 },
    { "on_ms": 600, "off_ms": 0   }
  ]
}
```

```bash
# SOS on the buzzer
curl -X POST http://jetson:3000/buzzer/sequence \
  -H 'Content-Type: application/json' \
  -d '{
    "steps": [
      {"on_ms":100,"off_ms":100}, {"on_ms":100,"off_ms":100}, {"on_ms":100,"off_ms":300},
      {"on_ms":300,"off_ms":100}, {"on_ms":300,"off_ms":100}, {"on_ms":300,"off_ms":300},
      {"on_ms":100,"off_ms":100}, {"on_ms":100,"off_ms":100}, {"on_ms":100,"off_ms":0}
    ]
  }'
```

---

## Suggested robot state conventions

| Robot state         | Light config                                    |
|---------------------|-------------------------------------------------|
| Booting             | Yellow ON (set at startup automatically)        |
| Ready / idle        | Green ON                                        |
| Busy / working      | Green slow blink (200ms on / 1800ms off)        |
| Warning             | Yellow blink (500ms/500ms)                      |
| Error               | Red ON                                          |
| Critical / fault    | Red fast blink (100ms/100ms) + buzzer pulse     |
| E-stop              | Red ON + Yellow ON + buzzer ON                  |
| Comms lost          | Yellow fast blink (100ms/100ms)                 |

---

## Environment variables

| Variable     | Default         | Description           |
|--------------|-----------------|-----------------------|
| `TOWER_BIND` | `0.0.0.0:3000`  | Listen address        |
| `RUST_LOG`   | `tower_api=info`| Log level             |
