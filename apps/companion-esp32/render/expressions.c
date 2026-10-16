#include "expressions.h"
#include <limits.h>
#include <math.h>

enum { CONSTANT = 1, VARIABLE, ADD, SUBTRACT, MULTIPLY, DIVIDE,
    MINIMUM, MAXIMUM, LESS, EQUAL, AND, OR, NOT, SELECT, CLAMP, LOOKUP };

static uint16_t u16(const uint8_t *bytes) {
    return (uint16_t)((uint16_t)bytes[0] | (uint16_t)bytes[1] << 8);
}
static int32_t i32(const uint8_t *bytes) {
    const uint32_t raw = (uint32_t)bytes[0] | (uint32_t)bytes[1] << 8 |
        (uint32_t)bytes[2] << 16 | (uint32_t)bytes[3] << 24;
    return raw <= INT32_MAX ? (int32_t)raw : -1 - (int32_t)(UINT32_MAX - raw);
}
static bool boolean(int32_t value) { return value == 0 || value == OSG_EXPRESSION_Q; }
static osg_expression_status checked(int64_t value, int32_t *output) {
    if (value < INT32_MIN || value > INT32_MAX) return OSG_EXPRESSION_OVERFLOW;
    *output = (int32_t)value; return OSG_EXPRESSION_OK;
}
osg_expression_status osg_expression_from_number(double value, int32_t *output) {
    if (!output) return OSG_EXPRESSION_ARGUMENT;
    *output = 0;
    if (!isfinite(value)) return OSG_EXPRESSION_INVALID_VALUE;
    const double scaled = value * OSG_EXPRESSION_Q;
    if (!isfinite(scaled)) return OSG_EXPRESSION_OVERFLOW;
    const double truncated = trunc(scaled);
    if (truncated < INT32_MIN || truncated > INT32_MAX) return OSG_EXPRESSION_OVERFLOW;
    *output = (int32_t)truncated; return OSG_EXPRESSION_OK;
}
static bool range(uint32_t offset, uint32_t count, uint32_t stride, uint32_t bytes) {
    return offset <= bytes && count <= (bytes - offset) / stride;
}
static osg_expression_status lookup(const osg_expression_program *program,
    uint16_t first, int32_t count, int32_t input, int32_t *output) {
    if (count < 2 || (uint32_t)count > OSG_EXPRESSION_MAX_LOOKUP_POINTS || first > program->lookup_count ||
        (uint32_t)count > program->lookup_count - first) return OSG_EXPRESSION_MALFORMED;
    int32_t previous_x = 0, previous_y = 0, result = 0;
    bool selected = false;
    for (uint32_t index = 0; index < (uint32_t)count; index++) {
        uint8_t record[8];
        if (!program->read(program->context, program->lookup_offset + ((uint32_t)first + index) * 8,
            record, sizeof(record))) return OSG_EXPRESSION_READ;
        const int32_t x = i32(record), y = i32(record + 4);
        if (index && x <= previous_x) return OSG_EXPRESSION_MALFORMED;
        if (!index && input <= x) { result = y; selected = true; }
        else if (!selected && input <= x) {
            /* Differences can span the entire u32 range. Their unsigned
             * product fits u64 but can exceed INT64_MAX. Divide first, then
             * restore the sign to match TS truncation toward zero. */
            const int64_t dy = (int64_t)y - previous_y;
            const uint64_t distance = (uint64_t)((int64_t)input - previous_x);
            const uint64_t span = (uint64_t)((int64_t)x - previous_x);
            const uint64_t magnitude = (uint64_t)(dy < 0 ? -dy : dy);
            const uint64_t amount = distance * magnitude / span;
            const int64_t value = (int64_t)previous_y + (dy < 0 ? -(int64_t)amount : (int64_t)amount);
            osg_expression_status status = checked(value, &result);
            if (status != OSG_EXPRESSION_OK) return status;
            selected = true;
        }
        previous_x = x; previous_y = y;
    }
    *output = selected ? result : previous_y; return OSG_EXPRESSION_OK;
}

osg_expression_status osg_expression_evaluate(const osg_expression_program *program,
    const osg_expression_variable *variables, uint16_t variable_count, int32_t *output) {
    if (output) *output = 0;
    if (!program || !program->read || !output || (variable_count && !variables) ||
        !program->instruction_count || program->instruction_count > OSG_EXPRESSION_MAX_INSTRUCTIONS ||
        program->lookup_count > UINT16_MAX || program->instruction_offset % 4 ||
        (program->lookup_count && program->lookup_offset % 4) ||
        !range(program->instruction_offset, program->instruction_count, 16, program->object_bytes) ||
        (program->lookup_count && !range(program->lookup_offset, program->lookup_count, 8, program->object_bytes)))
        return OSG_EXPRESSION_ARGUMENT;
    int32_t stack[OSG_EXPRESSION_MAX_STACK];
    unsigned depth = 0;
    for (uint32_t index = 0; index < program->instruction_count; index++) {
        uint8_t instruction[16];
        if (!program->read(program->context, program->instruction_offset + index * 16,
            instruction, sizeof(instruction))) return OSG_EXPRESSION_READ;
        const unsigned op = instruction[0];
        const uint16_t operand = u16(instruction + 2);
        const int32_t value = i32(instruction + 4);
        if (op < CONSTANT || op > LOOKUP || instruction[1] || i32(instruction + 8) || i32(instruction + 12))
            return OSG_EXPRESSION_MALFORMED;
        if ((op == CONSTANT && operand) || (op == VARIABLE && value) ||
            (op != CONSTANT && op != VARIABLE && op != LOOKUP && (operand || value)))
            return OSG_EXPRESSION_MALFORMED;
        const unsigned inputs = op == CONSTANT || op == VARIABLE ? 0 :
            op == NOT || op == LOOKUP ? 1 : op == SELECT || op == CLAMP ? 3 : 2;
        if (depth < inputs || depth - inputs >= OSG_EXPRESSION_MAX_STACK)
            return OSG_EXPRESSION_MALFORMED;
        int32_t result = 0;
        osg_expression_status status = OSG_EXPRESSION_OK;
        if (op == CONSTANT) result = value;
        else if (op == VARIABLE) {
            if (operand >= variable_count) return OSG_EXPRESSION_MALFORMED;
            const osg_expression_variable *sample = &variables[operand];
            if (!sample->available) return OSG_EXPRESSION_UNAVAILABLE;
            if (sample->type == OSG_EXPRESSION_BOOLEAN) {
                if (sample->value != 0 && sample->value != 1) return OSG_EXPRESSION_INVALID_BOOLEAN;
                result = sample->value == 1 ? OSG_EXPRESSION_Q : 0;
            } else if (sample->type == OSG_EXPRESSION_NUMBER) status = osg_expression_from_number(sample->value, &result);
            else return OSG_EXPRESSION_INVALID_VALUE;
        } else if (op == NOT) {
            const int32_t input = stack[depth - 1];
            if (!boolean(input)) return OSG_EXPRESSION_INVALID_BOOLEAN;
            result = input ? 0 : OSG_EXPRESSION_Q;
        } else if (op == SELECT) {
            const int32_t condition = stack[depth - 3];
            if (!boolean(condition)) return OSG_EXPRESSION_INVALID_BOOLEAN;
            result = condition ? stack[depth - 2] : stack[depth - 1];
        } else if (op == CLAMP) {
            const int32_t input = stack[depth - 3], min = stack[depth - 2], max = stack[depth - 1];
            if (min > max) return OSG_EXPRESSION_MALFORMED;
            result = input < min ? min : input > max ? max : input;
        } else if (op == LOOKUP) status = lookup(program, operand, value, stack[depth - 1], &result);
        else {
            const int32_t left = stack[depth - 2], right = stack[depth - 1];
            switch (op) {
            case ADD: status = checked((int64_t)left + right, &result); break;
            case SUBTRACT: status = checked((int64_t)left - right, &result); break;
            case MULTIPLY: status = checked((int64_t)left * right / OSG_EXPRESSION_Q, &result); break;
            case DIVIDE:
                if (!right) return OSG_EXPRESSION_DIVISION_BY_ZERO;
                status = checked((int64_t)left * OSG_EXPRESSION_Q / right, &result); break;
            case MINIMUM: result = left < right ? left : right; break;
            case MAXIMUM: result = left > right ? left : right; break;
            case LESS: result = left < right ? OSG_EXPRESSION_Q : 0; break;
            case EQUAL: result = left == right ? OSG_EXPRESSION_Q : 0; break;
            case AND:
            case OR:
                if (!boolean(left) || !boolean(right)) return OSG_EXPRESSION_INVALID_BOOLEAN;
                result = (op == AND ? left && right : left || right) ? OSG_EXPRESSION_Q : 0; break;
            }
        }
        if (status != OSG_EXPRESSION_OK) return status;
        depth -= inputs; stack[depth++] = result;
    }
    if (depth != 1) return OSG_EXPRESSION_MALFORMED;
    *output = stack[0]; return OSG_EXPRESSION_OK;
}