# Security policy

Use [GitHub Security](https://github.com/Viujnik/FuRy/security) to report a suspected vulnerability privately. If that page has no private reporting option, ask the maintainer for a private channel. Include the revision, the affected component, and the expected and observed behavior. For example, describe a decoder accepting a response beyond its documented limit using a synthetic response. Use synthetic data and leave production credentials, records, and account details out of the report.

## Trust the build inputs and bound the request

The CLI imports the Python file you give it. Run it on source you trust, such as your own checked-in models. Publish the generated registry from that build with the matching client.

Enforce your own transport limit before reading a response into memory. For example, an endpoint that serves at most 50 users can bind a SQL limit of 50 and reject an HTTP body above the size expected for that page. The codec also checks format and resource limits after receiving bytes.

Use bound SQL arguments for values such as an organization ID, and check access to that organization in the application. For remote PostgreSQL, configure the CA and use `sslmode=verify-full` to verify the server identity. [Database API](fury-py/README.md)
