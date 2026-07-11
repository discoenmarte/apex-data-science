FROM alpine:latest

# Instalar openconnect de una vez
RUN apk add --no-cache openconnect\
	# Instalar socat para port forwarding 
	&& apk add --no-cache socat \
	&& rm -rf /var/cache/apk/*
RUN apk add --no-cache postgresql-client
# Script de inicio
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]