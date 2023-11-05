# Replicated Log `v1`
**A simple leader-worker application for replicating messages**\
**received by a leader to all connected workers using**\
**REST and RPC written in Rust**

## Build
1. ``cd replog/``
2. ``docker-compose -f docker-compose.yml up --build`` + ``-d`` for a detached mode



## Usage
There is no UI/CLI available for the tool, but you can interact using REST API
___
### For service ``master``
#### ```http://localhost:8080```
#### ``POST /api/v1/messages`` - create a message
```
{
    "message": "#your-message"
    "wc": 3    // write concern
}
```

#### ``GET /api/v1/messages`` - get all messages
___
### For service ``secondary`` with ``n`` instances
#### ```http://localhost:808(1..n)```
#### ``GET /api/v1/messages`` - get all messages


## Done
### `v1`
- after each ``POST`` request, the message is replicated on every ``secondary`` server
- ``master`` ensures that ``secondary`` have received a message via ``ACK``
- ``master`` ``POST`` request are finished only after receiving ``ACK`` from all ``secondary`` (blocking replication approach)
- to test that the replication is blocking, an async delay has been added on the ``secondary``
- **RPC** and **REST** frameworks are used for ``master <-> secondary`` communication
- the implementation supports logging 
- ``master`` and ``secondary`` are encapsulated in **Docker**

### `v2`
+ client ``POST`` request in addition to the message also may contain `WRITE CONCERN` parameter `wc: 1..n`