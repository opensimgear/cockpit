import { EXPR } from "./abi.ts";
import { assert } from "./validation.ts";

export const Q = 65536;
const MIN = -2147483648n, MAX = 2147483647n;
function checked(value: bigint): number { assert(value >= MIN && value <= MAX, "Fixed-point overflow"); return Number(value); }
function scalar(value: number): number { assert(Number.isInteger(value) && value >= Number(MIN) && value <= Number(MAX), "Invalid fixed-point scalar"); return value; }
function boolean(value: number): boolean { assert(value === 0 || value === Q, "Invalid boolean scalar"); return value === Q; }
export function toFixed(value: number): number {
  assert(Number.isFinite(value), "Non-finite variable"); return checked(BigInt(Math.trunc(value * Q)));
}
export function formatFixed(value: number, precision: number): string {
  scalar(value); assert(Number.isInteger(precision) && precision >= 0 && precision <= 4, "Invalid precision");
  const negative = value < 0, factor = 10n ** BigInt(precision);
  // Decimal formatting rounds nearest, half away from zero, and never emits negative zero.
  const rounded = (BigInt(Math.abs(value)) * factor + BigInt(Q / 2)) / BigInt(Q);
  const sign = negative && rounded !== 0n ? "-" : "";
  return sign + (rounded / factor).toString() + (precision ? "." + (rounded % factor).toString().padStart(precision, "0") : "");
}
export interface Instruction { op: number; operand?: number; value?: number }
export type LookupPoint = readonly [number, number];
export type ScalarRange = readonly [number, number];
export type ScalarKind = "boolean" | "number";
/** Proves boolean membership separately from numeric min/max (0..1 is not necessarily a boolean). */
export function expressionKind(instructions: readonly Instruction[], variables: readonly ScalarKind[]): ScalarKind | null {
  try {
    expressionStackDepth(instructions); const stack: ScalarKind[] = [];
    for (const { op, operand = 0, value = 0 } of instructions) {
      if (op === EXPR.constant) stack.push(value === 0 || value === Q ? "boolean" : "number");
      else if (op === EXPR.variable) { assert(variables[operand], "Missing variable kind"); stack.push(variables[operand]); }
      else if (op === EXPR.not) { assert(stack.pop() === "boolean", "Boolean operand required"); stack.push("boolean"); }
      else if (op === EXPR.lookup) { stack.pop(); stack.push("number"); }
      else if (op === EXPR.select) { const otherwise = stack.pop(), whenTrue = stack.pop(); assert(stack.pop() === "boolean", "Boolean selector required"); stack.push(otherwise === "boolean" && whenTrue === "boolean" ? "boolean" : "number"); }
      else if (op === EXPR.clamp) { stack.pop(); stack.pop(); stack.pop(); stack.push("number"); }
      else {
        const right = stack.pop(), left = stack.pop();
        if (op === EXPR.and || op === EXPR.or) { assert(left === "boolean" && right === "boolean", "Boolean operands required"); stack.push("boolean"); }
        else stack.push(op === EXPR.less || op === EXPR.equal ? "boolean" : "number");
      }
    }
    return stack[0];
  } catch { return null; }
}
export function expressionStackDepth(instructions: readonly Instruction[]): number {
  assert(instructions.length > 0 && instructions.length <= 1024, "Expression instruction budget exceeded");
  let depth = 0, maximum = 0;
  for (const instruction of instructions) {
    const op = instruction.op;
    assert(Number.isInteger(op) && op >= EXPR.constant && op <= EXPR.lookup, "Unknown expression opcode");
    const input = op === EXPR.constant || op === EXPR.variable ? 0 : op === EXPR.not || op === EXPR.lookup ? 1 : op === EXPR.select || op === EXPR.clamp ? 3 : 2;
    assert(depth >= input, "Expression stack underflow"); depth = depth - input + 1; maximum = Math.max(maximum, depth);
    assert(maximum <= 8, "Expression stack depth exceeded");
  }
  assert(depth === 1, "Expression must return one scalar"); return maximum;
}
/** Invalid data produces null; it cannot manufacture a value or a gameplay input. */
export function evaluate(instructions: readonly Instruction[], variables: readonly (number | null)[], lookup: readonly LookupPoint[] = []): number | null {
  try {
    expressionStackDepth(instructions); const stack: number[] = [];
    const pop = () => scalar(stack.pop()!);
    for (const { op, operand = 0, value = 0 } of instructions) {
      if (op === EXPR.constant) stack.push(scalar(value));
      else if (op === EXPR.variable) { const input = variables[operand]; assert(input !== null && input !== undefined, "Unavailable variable"); stack.push(scalar(input)); }
      else if (op === EXPR.not) stack.push(boolean(pop()) ? 0 : Q);
      else if (op === EXPR.select) { const otherwise = pop(), whenTrue = pop(), condition = boolean(pop()); stack.push(condition ? whenTrue : otherwise); }
      else if (op === EXPR.clamp) { const max = pop(), min = pop(), input = pop(); assert(min <= max, "Reversed clamp"); stack.push(Math.max(min, Math.min(max, input))); }
      else if (op === EXPR.lookup) {
        const input = pop(); assert(Number.isInteger(operand) && Number.isInteger(value) && operand >= 0 && value >= 2 && value <= 256 && operand + value <= lookup.length, "Invalid lookup range");
        const points = lookup.slice(operand, operand + value);
        for (let index = 0; index < points.length; ++index) { scalar(points[index][0]); scalar(points[index][1]); if (index) assert(points[index][0] > points[index - 1][0], "Unsorted lookup"); }
        let result = points.at(-1)![1];
        if (input <= points[0][0]) result = points[0][1];
        else for (let index = 1; index < points.length; ++index) if (input <= points[index][0]) {
          const [x0, y0] = points[index - 1], [x1, y1] = points[index];
          result = checked(BigInt(y0) + BigInt(input - x0) * BigInt(y1 - y0) / BigInt(x1 - x0)); break;
        }
        stack.push(result);
      } else {
        const right = pop(), left = pop(); let result: number;
        switch (op) {
          case EXPR.add: result = checked(BigInt(left) + BigInt(right)); break;
          case EXPR.subtract: result = checked(BigInt(left) - BigInt(right)); break;
          case EXPR.multiply: result = checked(BigInt(left) * BigInt(right) / BigInt(Q)); break;
          case EXPR.divide: assert(right !== 0, "Division by zero"); result = checked(BigInt(left) * BigInt(Q) / BigInt(right)); break;
          case EXPR.minimum: result = Math.min(left, right); break;
          case EXPR.maximum: result = Math.max(left, right); break;
          case EXPR.less: result = left < right ? Q : 0; break;
          case EXPR.equal: result = left === right ? Q : 0; break;
          case EXPR.and: { const l = boolean(left), r = boolean(right); result = l && r ? Q : 0; break; }
          case EXPR.or: { const l = boolean(left), r = boolean(right); result = l || r ? Q : 0; break; }
          default: throw new Error("Unknown expression opcode");
        }
        stack.push(result);
      }
    }
    return stack[0];
  } catch { return null; }
}

/** Conservative interval proof for compiler admission. Unprovable/overflowing ranges fail closed. */
export function expressionRange(instructions: readonly Instruction[], variables: readonly ScalarRange[], lookup: readonly LookupPoint[] = []): ScalarRange | null {
  try {
    expressionStackDepth(instructions); const stack: ScalarRange[] = [];
    const push = (min: number, max = min) => { scalar(min); scalar(max); assert(min <= max, "Invalid interval"); stack.push([min, max]); };
    for (const { op, operand = 0, value = 0 } of instructions) {
      if (op === EXPR.constant) push(value);
      else if (op === EXPR.variable) { const range = variables[operand]; assert(range, "Missing variable range"); push(range[0], range[1]); }
      else if (op === EXPR.not) { stack.pop(); push(0, Q); }
      else if (op === EXPR.select) { const otherwise = stack.pop()!, whenTrue = stack.pop()!, condition = stack.pop()!; if (condition[0] === condition[1]) { const chosen = boolean(condition[0]) ? whenTrue : otherwise; push(chosen[0], chosen[1]); } else push(Math.min(otherwise[0], whenTrue[0]), Math.max(otherwise[1], whenTrue[1])); }
      else if (op === EXPR.clamp) { const max = stack.pop()!, min = stack.pop()!, input = stack.pop()!; assert(min[1] <= max[0], "Unprovable clamp interval"); push(Math.max(min[0], Math.min(max[0], input[0])), Math.max(min[1], Math.min(max[1], input[1]))); }
      else if (op === EXPR.lookup) {
        const input = stack.pop()!, at = (x: number) => evaluate([{ op: EXPR.constant, value: x }, { op: EXPR.lookup, operand, value }], [], lookup);
        const first = at(input[0]), last = at(input[1]); assert(first !== null && last !== null, "Invalid lookup interval");
        const values = [first, last, ...lookup.slice(operand, operand + value).filter(point => point[0] >= input[0] && point[0] <= input[1]).map(point => point[1])]; push(Math.min(...values), Math.max(...values));
      } else {
        const right = stack.pop()!, left = stack.pop()!;
        if (op >= EXPR.less && op <= EXPR.or) push(0, Q);
        else if (op === EXPR.add) push(checked(BigInt(left[0]) + BigInt(right[0])), checked(BigInt(left[1]) + BigInt(right[1])));
        else if (op === EXPR.subtract) push(checked(BigInt(left[0]) - BigInt(right[1])), checked(BigInt(left[1]) - BigInt(right[0])));
        else if (op === EXPR.minimum) push(Math.min(left[0], right[0]), Math.min(left[1], right[1]));
        else if (op === EXPR.maximum) push(Math.max(left[0], right[0]), Math.max(left[1], right[1]));
        else {
          if (op === EXPR.divide) assert(right[0] > 0 || right[1] < 0, "Division interval contains zero");
          const corners = left.flatMap(l => right.map(r => checked(op === EXPR.multiply ? BigInt(l) * BigInt(r) / BigInt(Q) : BigInt(l) * BigInt(Q) / BigInt(r)))); push(Math.min(...corners), Math.max(...corners));
        }
      }
    }
    return stack[0];
  } catch { return null; }
}
