PROFILE=client
DOCKER_COMPOSE_FILE=client/compose.yml

include .env/.$(PROFILE)
export

build:
	docker compose -f $(DOCKER_COMPOSE_FILE) build

vpn-connect:
	docker compose -f $(DOCKER_COMPOSE_FILE) run --rm --service-ports --name vpn-$(PROFILE) vpn-client

# No-interactivo: password por env, código 2FA depositado en TOKEN_FILE
# Uso: VPN_PASSWORD='...' make vpn-connect-auto   y luego: echo CODIGO > /tmp/vpn-2fa-token.txt
TOKEN_FILE ?= /tmp/vpn-2fa-token.txt
vpn-connect-auto:
	TOKEN_FILE=$(TOKEN_FILE) ./vpn-connect-auto.exp

start-forwarding:
	docker exec -d vpn-$(PROFILE) socat TCP-LISTEN:$(DB_PORT),fork,reuseaddr TCP:$(DB_HOST):$(DB_PORT)

blueprint:
	bash ./utils/public_schema_query.sh blueprint

table-sizes:
	bash ./utils/public_schema_query.sh table_sizes

total-size:
	bash ./utils/public_schema_query.sh total_size
