print("before:", type(Vector3))
Vector3 = { test = 1 }
print("after assign:", type(Vector3))
rawset(_G, "Vector3", { test2 = 2 })
print("after rawset:", type(Vector3), Vector3 and Vector3.test2)
print("_G.buffer test assign ok earlier")
