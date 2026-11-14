# Agent design

The current workflow selects relevant domain roles with deterministic routing. Mathematics is the baseline role; physics, chemistry, biology, and medicinal science are selected when matching question terms are present. In offline mode the activity status is `role_selected`, and the output says no model analysis ran. It is not reported as completed analysis.

When the optional hosted provider is configured, a single bounded request receives separate role instructions and returns a structured JSON analysis. The current Python application still owns specialist selection and this provider call; migrating decomposition, concurrency, provider adapters, and synthesis into Rust is follow-up architecture work. There are no recursive calls. Tool names are allowlisted by the Rust API before dispatch.
