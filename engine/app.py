"""Versioned, computation-only worker. No model transport or persistence lives here."""
import json
import math
import statistics
import sys

CONTRACT = "1.0"


def finite(value, name):
    result = float(value)
    if not math.isfinite(result):
        raise ValueError(f"{name} must be finite")
    return result


def tool(name, args):
    if not isinstance(args, dict):
        raise ValueError("Tool arguments must be an object")
    if name == "arithmetic":
        a, b = finite(args["a"], "a"), finite(args["b"], "b")
        op = args.get("op", "add")
        operations = {"add": lambda: a+b, "subtract": lambda: a-b, "multiply": lambda: a*b}
        if op == "divide":
            if b == 0: raise ValueError("division by zero")
            value = a/b
        elif op in operations: value = operations[op]()
        else: raise ValueError("unsupported arithmetic operation")
        if not math.isfinite(value): raise ValueError("result is not finite")
        return {"value": value, "operation": op, "epistemic_status": "calculated", "method": "IEEE-754 floating-point arithmetic"}
    if name == "power_law":
        xs = [finite(x, "x") for x in args["x"]]; ys = [finite(y, "y") for y in args["y"]]
        if len(xs) != len(ys) or len(xs) < 3 or any(x <= 0 for x in xs+ys): raise ValueError("Power-law fit requires at least 3 paired positive values")
        lx, ly = list(map(math.log, xs)), list(map(math.log, ys)); mx, my = statistics.mean(lx), statistics.mean(ly)
        denom = sum((x-mx)**2 for x in lx)
        if denom == 0: raise ValueError("power-law fit is singular: x values are identical")
        slope = sum((x-mx)*(y-my) for x,y in zip(lx,ly))/denom; intercept = my-slope*mx
        residual = sum((y-(intercept+slope*x))**2 for x,y in zip(lx,ly)); total=sum((y-my)**2 for y in ly)
        return {"coefficient": math.exp(intercept), "exponent": slope, "r_squared_log_space": 1-residual/total if total else 1.0, "epistemic_status":"calculated", "method":"ordinary least squares in log-log space"}
    if name == "polynomial":
        co=[finite(c,"coefficient") for c in args["coefficients"]]; x=finite(args.get("x",0),"x")
        if not co: raise ValueError("at least one coefficient is required")
        y=sum(c*x**(len(co)-i-1) for i,c in enumerate(co))
        if not math.isfinite(y): raise ValueError("polynomial result is not finite")
        return {"value":y,"epistemic_status":"calculated","method":"polynomial evaluation; coefficients highest degree first"}
    if name == "projectile":
        v=finite(args["speed"],"speed"); deg=finite(args["angle_degrees"],"angle_degrees"); g=finite(args.get("gravity",9.80665),"gravity")
        if v < 0 or g <= 0: raise ValueError("speed must be nonnegative and gravity positive")
        rad=math.radians(deg); return {"range_m":v*v*math.sin(2*rad)/g,"flight_time_s":2*v*math.sin(rad)/g,"max_height_m":v*v*math.sin(rad)**2/(2*g),"assumptions":["vacuum","level launch and landing","constant gravity"],"epistemic_status":"calculated","method":"ideal projectile equations; not experimental validation"}
    if name == "statistics":
        xs=[finite(x,"value") for x in args["values"]]
        if len(xs)<2: raise ValueError("At least two numeric values required")
        return {"count":len(xs),"mean":statistics.mean(xs),"sample_stdev":statistics.stdev(xs),"min":min(xs),"max":max(xs),"epistemic_status":"calculated","method":"Python descriptive statistics"}
    raise ValueError("Unknown scientific tool")


def main(route):
    payload=json.loads(sys.stdin.read() or "{}")
    if payload.get("contract_version") != CONTRACT: raise ValueError("Unsupported worker contract version")
    if route != "/api/tool": raise LookupError("Worker exposes computation tools only")
    result=tool(payload.get("name"),payload.get("args",{}))
    print(json.dumps({"contract_version":CONTRACT,"result":result},allow_nan=False))


if __name__ == "__main__":
    try: main(sys.argv[1] if len(sys.argv)>1 else "")
    except (ValueError,KeyError,TypeError,json.JSONDecodeError) as exc: print(json.dumps({"error":str(exc)})); sys.exit(2)
    except LookupError as exc: print(json.dumps({"error":str(exc)})); sys.exit(3)
    except Exception: print(json.dumps({"error":"Scientific worker failed unexpectedly"})); sys.exit(1)
