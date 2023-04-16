# Offload

A silly POC of a way to offload arbitrary tasks to a worker thread
(e.g., to serialize them without breaking out `async` machinery)
and await their results.
