# if / else / end — conditional branching

x = 10
y = 20

if x < y
    print("x is less than y")
else
    print("x is not less than y")
end

# Scalar: zero is false, nonzero is true
flag = 1
if flag
    print("flag is truthy")
end

# Nested conditions
function grade = classify(score)
    if score >= 90
        grade = "A"
    else
        if score >= 70
            grade = "B"
        else
            grade = "C"
        end
    end
end

print(classify(95))   # A
print(classify(75))   # B
print(classify(55))   # C
