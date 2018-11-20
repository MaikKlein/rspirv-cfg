# rspirv-cfg

## Why? 

This is a blend between `spirv-cfg` and `spirv-dis`. `spirv-cfg` often is hard to understand if the function contains a lot of branches, and `spirv-cfg` doesn't include any information about the code.
```
rspirv-cfg --file some.spv;dot -Tpng test.dot -O
```

![image](https://i.imgur.com/DHJFx38.png)
