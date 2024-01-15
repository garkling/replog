# Replicated Log `v3`
**A simple leader-worker application for replicating messages**\
**received by a leader to all connected workers using**\
**REST and RPC written in Rust**

## Build
1. ``cd replog/``
2. rename `config.env` into `.env` so `docker-compose.yml` could load envs, configure for yourself, do not forget about `secondary` launch section 
3. `source .env` if Linux or `$env:NTH=...; $env:SECONDARY_HOSTNAME=...; $env:SERVER_PORT=...1` if Windows
4. ``docker-compose up master`` + ``-d`` for a detached mode for a `master` node
5. ``docker run --rm -p 127.0.0.1:808${NTH}:${SERVER_PORT} --name "secondary${NTH}" --hostname "${SECONDARY_HOSTNAME}" --env-file ".env" --network replicated-log_default replicated-log-secondary:3.0.0`` (`docker-compose` doesn't want to nicely scale containers within the same service with different args, so this is the only approach I know without using `Swarm`/`K8S`) 
6. repeat the last step as many times as you want, with new args for every new instance

## Config
There is a configuration list (default values present in `config.env`):
#### `general`
+ `REPLICATION_DELAY_MS` - simulates a 'heavy I/O' replication time on `secondary`, you can set different values to check `wc`  
+ `REQUEST_TIMEOUT_MS` - replication request timeout
+ `RPC_SERVER_RECONNECT_DELAY_MS` - interval between RPC reconnect attempt on failure
#### `order correction`
+ `ORDER_CORRECTION_TIME_LIMIT_MS` - upper limit of waiting for the order correction in seconds in case of disordering
+ `ORDER_DIFF_MULTIPLIER` - a fraction from `ORDER_CORRECTION_TIME_LIMIT_MS` for every awaiting message adding to the total timeout in case of disordering
#### `circuit breaker`
+ `ABORT_CMD_TIMEOUT_MS` - how much time to wait on aborting the old heartbeat session after node recovered 
+ `STALL_NODE_LIFETIME_MS` - how much time to track a failed node in the hope of its recovery (default - `1h`)
#### `retries`
+ `MAX_RETRIES` - max attempt number global configuration
+ `BACKOFF_FACTOR` - multiplier of each next attempt's delay
+ `INIT_BACKOFF_MS` - initial delay between attempt 0 and 1
+ `MAX_BACKOFF_MS` - max allowable delay including jitter (default - `1h`)
#### `heartbeats`
+ `HB_FAIL_BUDGET` - failed requests allowed before the node is considered crashed/stall 
+ `HB_INTERVAL_MS` - interval between those requests
+ `HB_REQUEST_TIMEOUT_MS` - how much time to wait on response so after that consider the request as failed
+ `POST_FAIL_INTERVAL_MS` - a new interval for infrequent tracking of an unhealthy node
#### `quorum`
+ `WRITE_QUORUM` - the `N` of nodes to be alive and healthy in order to perform writes
#### `secondary launch`
+ `NTH` - `nth` consecutive node number, defines a hostname suffix and a next server port
+ `SECONDARY_HOSTNAME` - can be manually set, defaults to `secondary${NTH}`

The configuration can also be changed in `docker-compose.yml`'s `environment` section for `master`. 


## Usage
There is no UI/CLI available for the tool, but you can interact using REST API
___
### For service ``master``
#### ```http://localhost:8080```
#### ``POST /api/v1/messages`` - create a message
```
{
    "message": #your-message,
    "wc": 3,                // write concern

    "__ordering": 1,        // optional testing field, for 
                            // defining a custom message order 
                            // to check the guarantee of the correct order
}
```

#### ``GET /api/v1/messages`` - get all messages
___
### For service ``secondary`` with ``N`` instances
#### ```http://localhost:808(1..N)```
#### ``GET /api/v1/messages`` - get all messages
#### ``POST /api/v1/sabotage`` - a secret route for very untimely server errors, switches the sabotage mode `true/false`, defaults to `false`, throws an internal error at the end of the replication call :)


## Test
You can check heartbeats/retry logic by stopping or pausing a `secondary` container:
+ `docker stop {container_name}` or `Ctrl+C`
+ `docker pause/unpause {container_name}`

There are 3 health states of the node:
+ A `Healthy` node is active and has no limitations. It is checked every `HB_INTERVAL_MS` ms  
+ A `Suspected` node is not included in `WRITE QUORUM`. It becomes so after the first failed `HB`. Checked every `HB_INTERVAL_MS` ms
+ A `Failed` node is removed from the connections, however, current requests remain open. Acquires this state after exhausting the entire `HB_FAIL_BUDGET`. Checked every `POST_FAIL_INTERVAL_MS` ms

A `secondary` node will sync with the `master` on the first launch if any log diff present,  
or after recovery from `Failed` state while preserving its own log state. If latter, the `master` itself notifies  
the recovered node about its previous state and requests a sync + rejoin


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

### `v3`
+ `secondary` instance auto-joining/re-join to the cluster given `master` endpoint
+ `secondary` instance synchronization with the `master` node
+ heartbeats from `master` to `secondary` instances
+ a `retry` mechanism with configurable attempts and a linear `backoff` + `jitter`
+ a simple `WRITE QUORUM` for the `master`
+ auto-recover of replication server on `secondary` in case of failure


## Notes
There is no persistent storage, so all the state resets after restart.  
That's why it would be difficult to add UUIDs or sessions for nodes - currently the master's sync/breaker mechanisms assume that the same node will always have the same hostname)
