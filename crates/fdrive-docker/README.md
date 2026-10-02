Dropbox democratised the idea of a folder that syncs across your devices. Fdrive brings that same promise to agents running in containers. As the CEO of NVIDIA said:

> « when you deployed an agent ... the first thing you do is you take away all of its rights ... then you provision, you give it access to files ...»

Fdrive (the client/driver) and Filestash (the storage server) work together to create a shared filesystem to give your agents access to only the files they need, with [clear permission boundaries](https://www.filestash.app/docs/guide/authorization.html) and an [audit trail](https://www.filestash.app/img/screenshots/feature_audit.png) showing what your agents tried to do, what was allowed, and what was blocked, using cryptographic hash chaining and trusted timestamping to make the logs tamper-evident.

# Example

```
docker plugin install machines/fdrive --alias fdrive
export FILESTASH_SERVER=https://demo.filestash.app
export FILESTASH_TOKEN=uKzArshpw49Pta2tJZmg1mywkHcmimpW4lCjtVDNTbUFpmN0W2PXajSRR_fA5VrRr4Ks1S5SHwn9YffL74qRrVr1jssRUCXp4_uZdItrYUhQegWAGh5xT45-DgHowJb5aFtO-nODOMpFa6Y84Sit7za3GyM1miEpYm0wWgVucCs4tA==
docker compose -f - up <<EOF
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
