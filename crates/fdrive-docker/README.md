```
export FILESTASH_SERVER=https://demo.filestash.app
export FILESTASH_TOKEN=uKzArshpw49Pta2tJZmg1mywkHcmimpW4lCjtVDNTbUFpmN0W2PXajSRR_fA5VrRr4Ks1S5SHwn9YffL74qRrVr1jssRUCXp4_uZdItrYUhQegWAGh5xT45-DgHowJb5aFtO-nODOMpFa6Y84Sit7za3GyM1miEpYm0wWgVucCs4tA==
docker compose -f - up << 'EOF'
services:
  agent:
    image: alpine
    command: ["ls", "-la", "/mnt"]
    volumes:
    - files:/mnt

volumes:
  files:
    driver: fdrive
    driver_opts:
      server: ${FILESTASH_SERVER}
      token: ${FILESTASH_TOKEN}
EOF
```
