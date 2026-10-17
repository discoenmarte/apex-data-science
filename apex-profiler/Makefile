VPN_DIR=../db-vpn-connect

## ---- VPN (delegado a db-vpn-connect) ----

vpn-build:
	$(MAKE) -C $(VPN_DIR) build

# Pide password de VPN y luego el código 2FA por consola. Dejar corriendo.
vpn-connect:
	$(MAKE) -C $(VPN_DIR) vpn-connect

# En OTRA terminal, cuando la VPN ya reciba paquetes:
vpn-forward:
	$(MAKE) -C $(VPN_DIR) start-forwarding

# Verifica que la VPN alcance el host de la DB (toma DB_HOST de db-vpn-connect/.env/.client)
vpn-test:
	docker exec vpn-client ping -c 3 $$(grep '^DB_HOST=' ../db-vpn-connect/.env/.client | cut -d= -f2)

## ---- Pipeline ----

# Levanta Neo4j (con plugin GDS)
up:
	docker compose up -d neo4j

# Compila y corre el perfilador paralelo
profile:
	docker compose run --rm profiler

# Perfilado sin Neo4j (solo export JSON)
profile-json:
	docker compose run --rm -e SKIP_NEO4J=true profiler

browser:
	open http://localhost:7474

logs:
	docker compose logs -f neo4j

down:
	docker compose down

clean:
	docker compose down -v

.PHONY: vpn-build vpn-connect vpn-forward vpn-test up profile profile-json browser logs down clean
