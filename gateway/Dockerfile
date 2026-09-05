FROM nginx:1.28-alpine
COPY gateway/nginx.conf /etc/nginx/nginx.conf
COPY gateway/default.conf.template /etc/nginx/templates/default.conf.template
COPY gateway/web/ /usr/share/nginx/html/
# Reuse the actual chain UIs, not divergent copies of their business logic.
COPY dockerizd_tron/app/web/index.html /usr/share/nginx/html/networks/tron/index.html
COPY dockerizd_ethereum/web/index.html /usr/share/nginx/html/networks/ethereum/index.html
USER 101:101
EXPOSE 8080
