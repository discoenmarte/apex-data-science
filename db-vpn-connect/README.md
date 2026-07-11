# Explorar VPN client

El objetivo de esta rama es explorar la base de datos detras de la VPN de client, y producir el blueprint requerido para pruebas posteriores
Despues de correr `make blueprint` explorar usando `draft.ipynb` y `data/*`

## Construir el contenedor

```bash
docker compose build
```

## Dar permisos de ejecución a los scripts

```bash
chmod -R +x entrypoint.sh ./utils/*.sh
```

## Uso del Makefile

El Makefile facilita la ejecución de las tareas principales del proyecto. Los comandos disponibles son:

- **Conectar a la VPN:**
  ```bash
  make vpn-connect
  ```
- **Iniciar port forwarding:**
  ```bash
  make start-forwarding
  ```
- **Obtener el blueprint del esquema público:**
  ```bash
  make blueprint
  ```
- **Obtener tamaños de tablas:**
  ```bash
  make table-sizes
  ```
- **Obtener tamaño total del esquema:**
  ```bash
  make total-size
  ```

Comienza con:

```bash
make vpn-connect
```

Luego de esto va a pedir el password por consola y luego el código del 2fa que llega al email.

## Inicializar port forwarding para conexiones fuera del contenedor

Esto se ejecuta solo cuando ya se tenga una conexión completa con el vpn y se esté recibiendo paquetes.

Puedes iniciar el port forwarding usando el Makefile:

```bash
make start-forwarding
```

## Consultas útiles dentro del contenedor

- **Ver rutas en VPN:**
  ```bash
  docker exec vpn-fortinet ip route
  ```
- **Ver interfaces VPN:**
  ```bash
  docker exec vpn-fortinet ip addr
  ```
- **Verificar que retorne paquetes al ip en cuestión (usa el DB_HOST de `.env/.client`):**
  ```bash
  docker exec vpn-fortinet ping -c 3 <DB_HOST>
  ```
