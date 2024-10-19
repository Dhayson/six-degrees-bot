// This module is a derivative work from the Rust standard library, under MIT license
/*
Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
*/

use std::collections::hash_map::Iter;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hash};

#[derive(Debug)]
pub struct IntersectionMap<'a, T: 'a, S: 'a, K: 'a> {
    // iterator of the first set
    iter: Iter<'a, T, S>,
    // the second set
    other: &'a HashMap<T, S, K>,
    // account for reversed parity
    parity: bool,
}

impl<'a, T, S, K> Iterator for IntersectionMap<'a, T, S, K>
where
    T: Eq + Hash,
    K: BuildHasher,
{
    type Item = (&'a T, &'a S, &'a S);

    #[inline]
    fn next(&mut self) -> Option<(&'a T, &'a S, &'a S)> {
        loop {
            let (key, val1) = self.iter.next()?;
            if self.other.contains_key(key) {
                if self.parity {
                    let val: (&'a T, &'a S, &'a S) = (key, self.other.get(key).unwrap(), val1);
                    return Some(val);
                } else {
                    let val: (&'a T, &'a S, &'a S) = (key, val1, self.other.get(key).unwrap());
                    return Some(val);
                }
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (_, upper) = self.iter.size_hint();
        (0, upper)
    }
}

impl<'a, T, S, K> IntersectionMap<'a, T, S, K>
where
    T: Eq + Hash,
    K: BuildHasher,
{
    pub fn collect_map(self) -> HashMap<&'a T, (&'a S, &'a S)> {
        let mut map = HashMap::new();
        for (t, s1, s2) in self {
            map.insert(t, (s1, s2));
        }
        map
    }
}

/// It's a function that is missing in HashMap but that exists in HashSet.
///
/// Therefore, it's based on HashSet::intersection that existsis rust stdlib and modified to be useful
/// for hashmaps, returning the values in either hashmap.
///
/// However, it only supports that both hashmaps
/// have the same types.
pub fn intersection_map<'a, T, S, K>(
    first: &'a HashMap<T, S, K>,
    other: &'a HashMap<T, S, K>,
) -> IntersectionMap<'a, T, S, K> {
    if first.len() <= other.len() {
        IntersectionMap {
            iter: first.iter(),
            other,
            parity: false,
        }
    } else {
        IntersectionMap {
            iter: other.iter(),
            other: first,
            parity: true,
        }
    }
}
