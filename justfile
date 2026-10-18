# nerve-centre: build, deploy and run commands. Run `just` to list them.

set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

pi := "dylan@192.168.1.252"
target := "aarch64-unknown-linux-gnu"
services := "poller-tfl poller-weather"

# List the available commands
default:
    @just --list

# Run every test in the workspace
test:
    cargo test --workspace

# Run a service on this machine, against the dev topics
run service:
    cargo run -p {{service}} -- config/dev.toml

# Build a service for the Pi
build service: check-docker
    cross build --release --target {{target}} -p {{service}}

# Build a service, copy it to the Pi with its unit and config, and restart it
deploy service: (build service)
    scp target/{{target}}/release/{{service}} deploy/systemd/{{service}}.service deploy/config/{{service}}.toml {{pi}}:/tmp/
    ssh -t {{pi}} "sudo install -m 755 /tmp/{{service}} /opt/nerve-centre/bin/ && sudo install -m 644 /tmp/{{service}}.service /etc/systemd/system/ && sudo install -o root -g nerve -m 640 /tmp/{{service}}.toml /etc/nerve-centre/ && rm /tmp/{{service}} /tmp/{{service}}.service /tmp/{{service}}.toml && sudo systemctl daemon-reload && sudo systemctl enable {{service}} && sudo systemctl restart {{service}}"

# Deploy every service
deploy-all: (deploy "poller-tfl") (deploy "poller-weather")

# Show a service's recent log on the Pi. Add -f to keep following it
logs service *args:
    ssh {{pi}} journalctl -u {{service}} -n 50 --no-pager {{args}}

# Show whether the broker and every service are running on the Pi
status:
    ssh {{pi}} systemctl is-active mosquitto {{services}}

# Stop early with a clear message if Docker Desktop's engine isn't running
[private]
check-docker:
    @docker version --format "{{{{.Server.Version}}" 2>$null | Out-Null; if ($LASTEXITCODE -ne 0) { Write-Host "Docker isn't running: start Docker Desktop and wait for 'Engine running'" -ForegroundColor Red; exit 1 }
    