import math


def find_dfs_expressions(nums, target):
    results = []

    def dfs(current_nums, exprs):
        if len(current_nums) == 1:
            if math.isclose(current_nums[0], target, abs_tol=1e-6):
                results.append(exprs[0])
            return

        n = len(current_nums)
        for i in range(n):
            for j in range(i + 1, n):
                a, b = current_nums[i], current_nums[j]
                expr_a, expr_b = exprs[i], exprs[j]

                remaining = [current_nums[k] for k in range(n) if k != i and k != j]
                remaining_exprs = [exprs[k] for k in range(n) if k != i and k != j]

                operations = [
                    (a + b, f"({expr_a} + {expr_b})"),
                    (a - b, f"({expr_a} - {expr_b})"),
                    (b - a, f"({expr_b} - {expr_a})"),
                    (a * b, f"({expr_a} * {expr_b})"),
                ]

                if b != 0:
                    operations.append((a / b, f"({expr_a} / {expr_b})"))
                if a != 0:
                    operations.append((b / a, f"({expr_b} / {expr_a})"))

                for val, expr_str in operations:
                    dfs(remaining + [val], remaining_exprs + [expr_str])

    # Initial expression strings are just the numbers
    dfs(nums, [str(num) for num in nums])
    return results


results = find_dfs_expressions([9, 10, 9, 9, 1], 24)
for r in results:
    print(r)
print(f"There are {len(results)} solutions")
