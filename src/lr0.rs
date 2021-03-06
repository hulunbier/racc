use grammar::Grammar;
use closure::set_first_derives;
use closure::closure;
use util::Bitv32;
use std::collections::Bitv;

/// the structure of the LR(0) state machine
pub struct Core
{
    pub accessing_symbol: uint,
    pub items: Vec<i16>,
}

/// The structure used to record shifts
pub struct Shifts
{
    pub state: uint,
    pub shifts: Vec<i16>,
}

/// the structure used to store reductions
pub struct Reductions
{
    pub state: uint,
    pub rules: Vec<i16>,
}

#[deriving(Default)]
pub struct LR0Output
{
    pub states: Vec<Core>,
    pub shifts: Vec<Shifts>,
    pub reductions: Vec<Reductions>,
    pub nullable: Bitv,
    pub derives: Vec<i16>,
    pub derives_rules: Vec<i16>
}

impl LR0Output
{
    pub fn nstates(&self) -> uint {
        self.states.len()
    }
}

// intermediate variables for LR(0)
struct LR0State<'a>
{
    gram: &'a Grammar,

    // Contains the set of states that are relevant for each item.  Each entry in this
    // table corresponds to an item, so state_set.len() = nitems.  The contents of each
    // entry is a list of state indices (into LR0Output.states).
    state_set: Vec<Vec<uint>>, 
    
    states: Vec<Core>,

    kernel_base: Vec<i16>,      // values in this array are indexes into the kernel_items array    
    kernel_end: Vec<i16>,       // values in this array are indexes into the kernel_items array
    kernel_items: Vec<i16>,
}

fn sort_shift_symbols(shift_symbol: &mut [i16]) {
    // this appears to be a bubble-sort of shift_symbol?
    for i in range(1, shift_symbol.len()) {
        let symbol = shift_symbol[i];
        let mut j = i;
        while j > 0 && (shift_symbol[j - 1]) > symbol {
            shift_symbol[j] = shift_symbol[j - 1];
            j -= 1;
        }
        shift_symbol[j] = symbol;
    }
}

// shift_symbol contains a list of symbols.  it will be sorted.
// shiftset is empty when called, and will be filled with the states that correspond
// to the given shifted symbols.
fn append_states(lr0: &mut LR0State, shiftset: &mut Vec<i16>, shift_symbol: &Vec<i16>)
{
    assert!(shiftset.len() == 0);

    for i in range(0, shift_symbol.len()) {
        let symbol = shift_symbol[i] as uint;
        let state = get_state(lr0, symbol) as i16;
        shiftset.push(state as i16);
    }

    assert!(shiftset.len() == shift_symbol.len());
}

pub fn compute_lr0(gram: &Grammar) -> LR0Output
{
    let (derives, derives_rules) = set_derives(gram);

    // was: allocate_item_sets()
    // This defines LR0State fields: kernel_base, kernel_items, kernel_end, shift_symbol
    // The kernel_* fields are allocated to well-defined sizes, but their contents are
    // not well-defined yet.
    let mut kernel_items_count: uint = 0;
    let mut symbol_count: Vec<i16> = Vec::from_elem(gram.nsyms, 0);
    for i in range(0, gram.nitems) {
        let symbol = gram.ritem[i];
        if symbol >= 0 {
            kernel_items_count += 1;
            symbol_count[symbol as uint] += 1;
        }
    }
    let kernel_base = {
        let mut kernel_base: Vec<i16> = Vec::from_elem(gram.nsyms, 0);
        let mut count: uint = 0;
        for i in range(0, gram.nsyms) {
            kernel_base[i] = count as i16;
            count += symbol_count[i] as uint;
        }
        kernel_base
    };

    let mut lr0: LR0State = LR0State {
        gram: gram,
        state_set: Vec::from_fn(gram.nitems, |_| Vec::new()),
        kernel_base: kernel_base,
        kernel_end: Vec::from_elem(gram.nsyms, -1),
        kernel_items: Vec::from_elem(kernel_items_count, 0),
        states: initialize_states(gram, derives.as_slice(), derives_rules.as_slice())
    };

    let first_derives = set_first_derives(gram, derives.as_slice(), derives_rules.as_slice());

    // These vectors are used for building tables during each state.
    // It is inefficient to allocate and free these vectors within
    // the scope of processing each state.
    let mut red_set: Vec<i16> = Vec::new();
    let mut shift_set: Vec<i16> = Vec::with_capacity(gram.nsyms);
    let mut item_set: Vec<i16> = Vec::with_capacity(gram.nitems);
    let mut rule_set: Bitv32 = Bitv32::from_elem(gram.nrules, false);
    let mut shift_symbol: Vec<i16> = Vec::new();

    // this_state represents our position within our work list.  The output.states
    // array represents both our final output, and this_state is the next state
    // within that array, where we need to generate new states from.  New states
    // are added to output.states within append_states().
    let mut this_state: uint = 0;

    // State which becomes the output
    let mut reductions: Vec<Reductions> = Vec::new();
    let mut shifts: Vec<Shifts> = Vec::new();

    while this_state < lr0.states.len() {
        assert!(item_set.len() == 0);
        debug!("computing closure for state s{}:", this_state);
        print_core(gram, this_state, &lr0.states[this_state]);

        // The output of closure() is stored in item_set.
        // rule_set is used only as temporary storage.
        // debug!("    nucleus items: {}", lr0.states[this_state].items.as_slice());
        closure(gram, lr0.states[this_state].items.as_slice(), &first_derives, gram.nrules, &mut rule_set, &mut item_set);

        // The output of save_reductions() is stored in reductions.
        // red_set is used only as temporary storage.
        save_reductions(gram, this_state, item_set.as_slice(), &mut red_set, &mut reductions);

        // new_item_sets updates kernel_items, kernel_end, and shift_symbol, and also
        // computes (returns) the number of shifts for the current state.
        debug!("    new_item_sets: item_set = {}", item_set);
        new_item_sets(gram, &mut lr0, item_set.as_slice(), &mut shift_symbol);
        sort_shift_symbols(shift_symbol.as_mut_slice());

        // append_states() potentially adds new states to lr0.states
        append_states(&mut lr0, &mut shift_set, &shift_symbol);
        debug!("    shifts: {}", shift_set.as_slice());

        // If there are any shifts for this state, record them.
        if shift_symbol.len() > 0 {
            shifts.push(Shifts {
                state: this_state,
                shifts: vec_from_slice(shift_set.as_slice())
            });
        }

        item_set.clear();
        shift_set.clear();
        red_set.clear();
        shift_symbol.clear();

        debug!("");
        this_state += 1;
    }

    // Return results
    LR0Output {
        states: lr0.states,
        reductions: reductions,
        shifts: shifts,
        nullable: set_nullable(gram),
        derives: derives,
        derives_rules: derives_rules
    }
}

// Gets the state for a particular symbol.  If no appropriate state exists,
// then a new state will be created.
fn get_state(lr0: &mut LR0State, symbol: uint) -> uint
{
    let isp = lr0.kernel_base[symbol] as uint;
    let iend = lr0.kernel_end[symbol] as uint;
    let n = iend - isp;

    let key = lr0.kernel_items[isp] as uint; // key is an item index, in [0..nitems).

    // Search for an existing Core that has the same items.
    for &state in lr0.state_set[key].iter() {
        let sp_items = &lr0.states[state].items;
        if sp_items.len() == n {
            let mut found = true;
            for j in range(0, n) {
                if lr0.kernel_items[isp + j] != sp_items[j] {
                    found = false;
                    break;
                }
            }
            if found {
                // We found an existing state with the same items.
                return state;
            }
        }
    }

    // No match.  Add a new entry to the list.

    assert!(lr0.states.len() < 0x7fff);

    let new_state = lr0.states.len();
    lr0.states.push(Core {
        accessing_symbol: symbol,
        items: vec_from_slice(lr0.kernel_items.slice(lr0.kernel_base[symbol] as uint, lr0.kernel_end[symbol] as uint))
    });

    // Add the new state to the state set for this symbol.
    lr0.state_set[key].push(new_state);

    debug!("    created state s{}:", new_state);
    print_core(lr0.gram, new_state, &lr0.states[new_state]);

    new_state
}

// This function creates the initial state, using the DERIVES relation for
// the start symbol.  From this initial state, we will discover / create all
// other states, by examining a state, the next variables that could be
// encountered in those states, and finding the transitive closure over same.
// Initializes the state table.
fn initialize_states(gram: &Grammar, derives: &[i16], derives_rules: &[i16]) -> Vec<Core>
{
    debug!("initialize_states");

    let start_derives: uint = derives[gram.start_symbol] as uint;

    // measure the number of items in the initial state, so we can
    // allocate a vector of the exact size.
    let mut core_nitems: uint = 0;
    while derives_rules[start_derives + core_nitems] >= 0 {
        core_nitems += 1;
    }

    // create the initial state
    let mut states: Vec<Core> = Vec::new();
    states.push(Core {
        items: {
            let mut items = Vec::with_capacity(core_nitems);
            let mut i: uint = 0;
            while derives_rules[start_derives + i] >= 0 {
                items.push(gram.rrhs[derives_rules[start_derives + i] as uint]);
                i += 1;
            }
            items
        },
        accessing_symbol: 0
    });

    debug!("initial state:");
    print_core(gram, 0, &states[0]);

    states
}

fn print_core(gram: &Grammar, state: uint, core: &Core)
{
    debug!("    s{} : accessing_symbol={}", state, gram.name[core.accessing_symbol]);

    let mut line = String::new();
    for i in range(0, core.items.len()) {
        let rhs = core.items[i] as uint;
        line.push_str(format!("item {:4} : ", rhs).as_slice());

        // back up to start of this rule
        let mut rhs_first = rhs;
        while rhs_first > 0 && gram.ritem[rhs_first - 1] >= 0 {
            rhs_first -= 1;
        }

        // loop through rhs
        let mut j = rhs_first;
        while gram.ritem[j] >= 0 {
            if j == rhs {
                line.push_str(" .");
            }
            line.push(' ');
            line.push_str(gram.name[gram.ritem[j] as uint].as_slice());
            j += 1;
        }
        if j == rhs {
            line.push_str(" .");
        }

        debug!("        {}", line);
        line.clear();
    }
}

// fills shift_symbol with shifts
fn new_item_sets(gram: &Grammar, lr0: &mut LR0State, item_set: &[i16], shift_symbol: &mut Vec<i16>)
{
    assert!(shift_symbol.len() == 0);

    // reset kernel_end
    for i in lr0.kernel_end.iter_mut() {
        *i = -1;
    }

    for &it in item_set.iter() {
        let symbol = gram.ritem[it as uint];
        if symbol > 0 {
            let mut ksp = lr0.kernel_end[symbol as uint];
            if ksp == -1 {
                shift_symbol.push(symbol);
                ksp = lr0.kernel_base[symbol as uint];
            }

            lr0.kernel_items[ksp as uint] = (it + 1) as i16;
            ksp += 1;
            lr0.kernel_end[symbol as uint] = ksp;
        }
    }
}

fn save_reductions(gram: &Grammar, this_state: uint, item_set: &[i16], red_set: &mut Vec<i16>, reductions: &mut Vec<Reductions>)
{
    assert!(red_set.len() == 0);

    // Examine the items in the given item set.  If any of the items have reached the
    // end of the rhs list for a particular rule, then add that rule to the reduction set.
    // We discover this by testing the sign of the next symbol in the item; if it is
    // negative, then we have reached the end of the symbols on the rhs of a rule.  See
    // the code in reader::pack_grammar(), where this information is set up.
    for &i in item_set.iter() {
        assert!(i >= 0);
        let item = gram.ritem[i as uint];
        if item < 0 {
            let rule = (-item) as uint;
            debug!("        reduction: r{}  {}", rule, gram.rule_to_str(rule));
            red_set.push(-item);
        }
    }

    if red_set.len() != 0 {
        reductions.push(Reductions {
            state: this_state,
            rules: vec_from_slice(red_set.as_slice())
        });
        red_set.clear();
    }
    else {
        debug!("    no reductions");
    }
}

// Computes the "derives" and "derives_rules" arrays.
fn set_derives(gram: &Grammar) -> (Vec<i16>, Vec<i16>) // (derives, derives_rules)
{
    // note: 'derives' appears to waste its token space; consider adjusting indices
    // so that only var indices are used
	let mut derives: Vec<i16> = Vec::from_elem(gram.nsyms, 0);
    let mut derives_rules: Vec<i16> = Vec::with_capacity(gram.nvars + gram.nrules);

    for lhs in range(gram.start_symbol, gram.nsyms) {
        derives[lhs] = derives_rules.len() as i16;
        for r in range(0, gram.nrules) {
            if gram.rlhs[r] as uint == lhs {
                derives_rules.push(r as i16);
            }
        }
        derives_rules.push(-1);
    }

    print_derives(gram, derives.as_slice(), derives_rules.as_slice());

    (derives, derives_rules)
}

fn print_derives(gram: &Grammar, derives: &[i16], derives_rules: &[i16])
{
    debug!("");
    debug!("DERIVES:");
    debug!("");

    for lhs in range(gram.start_symbol, gram.nsyms) {
        debug!("    {} derives rules: ", gram.name[lhs]);
        let mut sp = derives[lhs] as uint;
        while derives_rules[sp] >= 0 {
            let r = derives_rules[sp] as uint;
            debug!("        {}", gram.rule_to_str(r).as_slice());
            sp += 1;
        }
    }
    debug!("");
}

fn set_nullable(gram: &Grammar) -> Bitv
{
    let mut nullable = Bitv::from_elem(gram.nsyms, false);

    let mut done_flag = false;
    while !done_flag {
        done_flag = true;
        let mut i = 1;
        while i < gram.nitems {
            let mut empty = true;
            let mut j: i16;
            loop {
                j = gram.ritem[i];
                if j < 0 {
                    break;
                }
                if !nullable[j as uint] {
                    empty = false;
                }
                i += 1;
            }
            if empty {
                j = gram.rlhs[(-j) as uint];
                if !nullable[j as uint] {
                    nullable.set(j as uint, true);
                    done_flag = false;
                }
            }
        	i += 1;
        }
    }

    for i in range(gram.start_symbol, gram.nsyms) {
        if nullable[i] {
            debug!("{} is nullable", gram.name[i]);
        }
        else {
            debug!("{} is not nullable", gram.name[i]);
        }
    }

    nullable
}

fn vec_from_slice<T:Clone>(s: &[T]) -> Vec<T>
{
	let mut v = Vec::with_capacity(s.len());
	v.push_all(s);
	v
}
