# Replicated Log `v1`
**A simple leader-worker application for replicating messages**\
**received by a leader to all connected workers using**\
**REST and RPC written in Rust**

## Build
1. ``cd replog/``
2. ``docker-compose -f docker-compose.yml up --build`` + ``-d`` for a detached mode

## Config
There is a small configuration list (going to be more flexible in `v3`):
+ `REPLICATION_DELAY: 5` - simulates a 'heavy I/O' replication time on `secondary`, you can set different values to check `wc`  
+ `ORDER_DIFF_MULTIPLIER: 0.2` - a fraction from `ORDER_CORRECTION_TIME_LIMIT_S` for every awaiting message adding to the total timeout in case of disordering
+ `ORDER_CORRECTION_TIME_LIMIT_S: 60` - upper limit of waiting for the order correction in seconds in case of disordering

## Usage
There is no UI/CLI available for the tool, but you can interact using REST API
___
### For service ``master``
#### ```http://localhost:8080```
#### ``POST /api/v1/messages`` - create a message
```
{
    "message": "#your-message",
    "wc": 3,                // write concern

    "__ordering": 1,        // optional testing field, for 
                            // defining a custom message order 
                            // to check the guarantee of the correct order

    "__duplicate": false    // optional testing field, 
                            // commands to simulate a message duplication
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
+ added a unique ID and an atomic ordering integer in the body of a replication message
+ added a replication state/message statuses structures for keeping the total ordering and message IDs 
+ added algorithms for message ordering correction and duplication checks

### `#TODO`
+ `secondary` instance auto-joining to the cluster given `master` endpoint
+ heartbeats from `master` to `secondary` instances
+ auto-recover of replication server on `secondary` in case of failure
+ features of the `v3` iteration