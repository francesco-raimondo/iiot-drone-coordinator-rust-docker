from fastapi import FastAPI

app = FastAPI(title="Drone Coordinator Observability Backend")


@app.get("/")
@app.get("/home")
def home():
    """Root/home status endpoint."""
    return {"message": "ok, funziono"}
