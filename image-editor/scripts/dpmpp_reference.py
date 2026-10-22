"""Regenerate independent Diffusers CPU oracle; no models or network access needed."""
import json
from pathlib import Path
import torch
import diffusers
from diffusers import DPMSolverMultistepScheduler

cases=[]
for steps,begin in [(1,0),(4,0),(8,0),(20,0),(50,0),(20,15),(150,137)]:
    scheduler=DPMSolverMultistepScheduler(beta_start=0.00085,beta_end=0.012,beta_schedule="scaled_linear",prediction_type="epsilon",algorithm_type="dpmsolver++",solver_order=2,solver_type="midpoint",use_karras_sigmas=True,final_sigmas_type="zero")
    scheduler.set_timesteps(steps)
    scheduler.set_begin_index(begin)
    sample=torch.tensor([[0.2,-0.4,0.7,1.0]],dtype=torch.float32)
    case={"steps":steps,"begin":begin,"timesteps":scheduler.timesteps.tolist(),"sigmas":scheduler.sigmas.tolist(),"outputs":[]}
    if begin:
        sample=scheduler.add_noise(sample,torch.tensor([[0.3,0.1,-0.2,0.5]],dtype=torch.float32),scheduler.timesteps[begin:begin+1])
        case["noised"]=sample.flatten().tolist()
    for i in range(begin,steps):
        epsilon=torch.full_like(sample,0.01*(i+1))
        sample=scheduler.step(epsilon,scheduler.timesteps[i],sample).prev_sample
        case["outputs"].append(sample.flatten().tolist())
    cases.append(case)
path=Path(__file__).resolve().parents[1]/"tests/fixtures/dpmpp_diffusers.json"
path.parent.mkdir(exist_ok=True)
path.write_text(json.dumps({"reference":"diffusers.DPMSolverMultistepScheduler","version":diffusers.__version__,"cases":cases},indent=2),encoding="utf-8")
print(path)
